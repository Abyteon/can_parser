use anyhow::{Result, anyhow};
use std::sync::Arc;
use rayon::prelude::*;
use tokio::task;
use tracing::{info, debug};

use crate::frame_parser::{ParsedFrame, FileLevelData};
use crate::memory_pool::MemoryPool;
use crate::dbc_parser::DbcParser;
use crate::file_cache::CachedFileReader;

/// 并发增强的帧解析器 - 在每一层都使用并发处理
pub struct ConcurrentFrameParser {
    memory_pool: Arc<MemoryPool>,
    dbc_parser: Arc<DbcParser>,
    file_reader: Option<CachedFileReader>,
    config: ConcurrentParseConfig,
}

/// 并发解析配置
#[derive(Debug, Clone)]
pub struct ConcurrentParseConfig {
    /// 解压缩并发数
    pub decompress_workers: usize,
    /// 解析层级并发数  
    pub parse_workers: usize,
    /// 帧处理并发数
    pub frame_workers: usize,
    /// 批处理大小
    pub batch_size: usize,
}

impl Default for ConcurrentParseConfig {
    fn default() -> Self {
        let cpu_count = num_cpus::get();
        Self {
            decompress_workers: cpu_count,      // 解压缩CPU密集
            parse_workers: cpu_count * 2,       // 解析相对轻量
            frame_workers: cpu_count * 2,       // 帧处理并发
            batch_size: 64,                     // 批处理大小
        }
    }
}

impl ConcurrentFrameParser {
    pub fn new(
        memory_pool: Arc<MemoryPool>, 
        dbc_parser: Arc<DbcParser>,
        file_reader: Option<CachedFileReader>,
        config: Option<ConcurrentParseConfig>
    ) -> Self {
        Self {
            memory_pool,
            dbc_parser,
            file_reader,
            config: config.unwrap_or_default(),
        }
    }

    /// 并发解析文件 - 主入口点
    pub async fn parse_file_concurrent(&self, file_path: &str) -> Result<Vec<ParsedFrame>> {
        debug!("🚀 开始并发解析文件: {}", file_path);
        
        // 1. 读取文件数据
        let file_data = self.read_file_data(file_path).await?;
        
        // 2. 并发解析第0层 - 文件级别的压缩数据块
        let compressed_blocks = self.parse_file_level_concurrent(&file_data).await?;
        debug!("📦 解析到 {} 个压缩数据块", compressed_blocks.len());
        
        // 3. 并发解压缩所有数据块
        let decompressed_blocks = self.decompress_blocks_concurrent(compressed_blocks).await?;
        debug!("🗜️  解压缩完成，得到 {} 个数据块", decompressed_blocks.len());
        
        // 4. 并发解析所有层级
        let all_frames = self.parse_all_layers_concurrent(decompressed_blocks).await?;
        
        info!("✅ 并发解析完成，共获得 {} 个帧", all_frames.len());
        Ok(all_frames)
    }

    /// 读取文件数据
    async fn read_file_data(&self, file_path: &str) -> Result<Vec<u8>> {
        if let Some(ref reader) = self.file_reader {
            if let Some(data) = reader.read_file(std::path::Path::new(file_path)).await? {
                Ok(data.to_vec())
            } else {
                Err(anyhow!("文件不存在: {}", file_path))
            }
        } else {
            Ok(tokio::fs::read(file_path).await?)
        }
    }

    /// 并发解析第0层 - 文件级别数据
    async fn parse_file_level_concurrent(&self, data: &[u8]) -> Result<Vec<FileLevelData>> {
        let mut _blocks: Vec<FileLevelData> = Vec::new();
        let mut offset = 0;

        // 首先顺序解析出所有块的边界
        let mut block_ranges = Vec::new();
        while offset < data.len() {
            if data.len() - offset < 35 {
                break;
            }
            
            // 读取数据长度
            let data_length = u32::from_be_bytes([
                data[offset + 31], data[offset + 32], 
                data[offset + 33], data[offset + 34]
            ]) as usize;
            
            if data.len() - offset < 35 + data_length {
                break;
            }
            
            block_ranges.push((offset, 35 + data_length));
            offset += 35 + data_length;
        }

        // 并发解析每个块
        let parsed_blocks: Result<Vec<_>, _> = block_ranges
            .into_par_iter()
            .map(|(start, length)| {
                self.parse_single_file_block(&data[start..start + length])
            })
            .collect();

        parsed_blocks
    }

    /// 解析单个文件级别数据块
    fn parse_single_file_block(&self, data: &[u8]) -> Result<FileLevelData> {
        if data.len() < 35 {
            return Err(anyhow!("数据长度不足"));
        }

        let mut header = [0u8; 35];
        header.copy_from_slice(&data[0..35]);

        let data_length = u32::from_be_bytes([
            data[31], data[32], data[33], data[34]
        ]);

        let compressed_data = data[35..].to_vec();

        Ok(FileLevelData {
            header,
            data_length,
            compressed_data,
        })
    }

    /// 并发解压缩所有数据块
    async fn decompress_blocks_concurrent(&self, blocks: Vec<FileLevelData>) -> Result<Vec<Vec<u8>>> {
        info!("🗜️  开始并发解压缩 {} 个数据块", blocks.len());
        
        // 使用tokio和rayon结合进行并发解压缩
        let decompressed_results: Result<Vec<_>, _> = task::spawn_blocking(move || {
            blocks
                .into_par_iter()
                .map(|block| {
                    Self::decompress_single_block_static(&block.compressed_data)
                })
                .collect()
        }).await?;

        decompressed_results
    }

    /// 解压缩单个数据块
    fn decompress_single_block_static(compressed_data: &[u8]) -> Result<Vec<u8>> {
        use flate2::read::GzDecoder;
        use std::io::Read;
        
        let mut decoder = GzDecoder::new(compressed_data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }

    /// 并发解析所有层级
    async fn parse_all_layers_concurrent(&self, decompressed_blocks: Vec<Vec<u8>>) -> Result<Vec<ParsedFrame>> {
        info!("⚙️  开始并发解析多层数据结构");
        
        // 使用流水线方式并发处理多个层级
        let all_frames_futures: Vec<_> = decompressed_blocks
            .into_iter()
            .map(|block| {
                let dbc_parser = self.dbc_parser.clone();
                task::spawn(async move {
                    Self::parse_block_layers_concurrent(block, dbc_parser).await
                })
            })
            .collect();

        // 等待所有并发任务完成
        let mut all_frames = Vec::new();
        for future in all_frames_futures {
            let block_frames = future.await??;
            all_frames.extend(block_frames);
        }

        Ok(all_frames)
    }

    /// 并发解析单个块的所有层级
    async fn parse_block_layers_concurrent(
        block_data: Vec<u8>, 
        dbc_parser: Arc<DbcParser>
    ) -> Result<Vec<ParsedFrame>> {
        
        // 第1层：并发解析帧序列
        let frame_sequences = Self::parse_frame_sequences_concurrent(&block_data).await?;
        
        // 第2层：并发解析帧集合
        let all_frame_data = Self::parse_frames_concurrent(frame_sequences).await?;
        
        // 第3层：并发解析单个帧
        let parsed_frames = Self::parse_single_frames_concurrent(all_frame_data, dbc_parser).await?;
        
        Ok(parsed_frames)
    }

    /// 并发解析第1层：帧序列数据
    async fn parse_frame_sequences_concurrent(data: &[u8]) -> Result<Vec<Vec<u8>>> {
        // 首先找到所有序列的边界
        let mut sequence_ranges = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            if data.len() - offset < 24 { // 20字节header + 4字节长度
                break;
            }

            let data_length = u32::from_be_bytes([
                data[offset + 20], data[offset + 21], 
                data[offset + 22], data[offset + 23]
            ]) as usize;

            if data.len() - offset < 24 + data_length {
                break;
            }

            sequence_ranges.push((offset + 24, data_length));
            offset += 24 + data_length;
        }

        // 并发提取所有序列数据
        let sequences: Vec<Vec<u8>> = sequence_ranges
            .into_par_iter()
            .map(|(start, length)| {
                data[start..start + length].to_vec()
            })
            .collect();

        Ok(sequences)
    }

    /// 并发解析第2层：帧集合数据
    async fn parse_frames_concurrent(sequences: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>> {
        let frame_futures: Vec<_> = sequences
            .into_iter()
            .map(|sequence| {
                task::spawn_blocking(move || {
                    Self::parse_frames_from_sequence(sequence)
                })
            })
            .collect();

        let mut all_frames = Vec::new();
        for future in frame_futures {
            let frames = future.await??;
            all_frames.extend(frames);
        }

        Ok(all_frames)
    }

    /// 从序列中解析帧集合
    fn parse_frames_from_sequence(sequence_data: Vec<u8>) -> Result<Vec<Vec<u8>>> {
        let mut frames = Vec::new();
        let mut offset = 0;

        while offset < sequence_data.len() {
            if sequence_data.len() - offset < 24 {
                break;
            }

            let data_length = u32::from_be_bytes([
                sequence_data[offset + 20], sequence_data[offset + 21], 
                sequence_data[offset + 22], sequence_data[offset + 23]
            ]) as usize;

            if sequence_data.len() - offset < 24 + data_length {
                break;
            }

            let frame_data = sequence_data[offset + 24..offset + 24 + data_length].to_vec();
            frames.push(frame_data);
            offset += 24 + data_length;
        }

        Ok(frames)
    }

    /// 并发解析第3层：单个帧
    async fn parse_single_frames_concurrent(
        frame_data_list: Vec<Vec<u8>>, 
        dbc_parser: Arc<DbcParser>
    ) -> Result<Vec<ParsedFrame>> {
        
        // 分批处理，避免创建过多任务
        let batch_size = 128;
        let mut all_frames = Vec::new();

        for batch in frame_data_list.chunks(batch_size) {
            let batch_futures: Vec<_> = batch
                .iter()
                .map(|frame_data| {
                    let dbc_parser = dbc_parser.clone();
                    let frame_data = frame_data.clone();
                    task::spawn_blocking(move || {
                        Self::parse_single_frame_sync(frame_data, dbc_parser)
                    })
                })
                .collect();

            // 等待当前批次完成
            for future in batch_futures {
                if let Ok(frame) = future.await? {
                    all_frames.push(frame);
                }
            }
        }

        Ok(all_frames)
    }

    /// 同步解析单个帧（在blocking线程池中运行）
    fn parse_single_frame_sync(frame_data: Vec<u8>, _dbc_parser: Arc<DbcParser>) -> Result<ParsedFrame> {
        // 简化的帧解析逻辑
        if frame_data.len() < 16 {
            return Err(anyhow!("帧数据长度不足"));
        }

        // 提取基本信息
        let timestamp = u64::from_le_bytes([
            frame_data[0], frame_data[1], frame_data[2], frame_data[3],
            frame_data[4], frame_data[5], frame_data[6], frame_data[7],
        ]);

        let can_id = u32::from_le_bytes([
            frame_data[8], frame_data[9], frame_data[10], frame_data[11]
        ]);

        let data_len = std::cmp::min(8, frame_data.len() - 12);
        let data = frame_data[12..12 + data_len].to_vec();

        // 使用DBC解析信号（如果可用）
        let signals = std::collections::HashMap::new(); // 简化处理

        Ok(ParsedFrame {
            timestamp,
            can_id,
            data,
            signals,
        })
    }

    /// 获取配置信息
    pub fn get_config(&self) -> &ConcurrentParseConfig {
        &self.config
    }

    /// 性能统计
    pub async fn get_performance_stats(&self) -> ConcurrentParseStats {
        ConcurrentParseStats {
            decompress_workers: self.config.decompress_workers,
            parse_workers: self.config.parse_workers,
            frame_workers: self.config.frame_workers,
            batch_size: self.config.batch_size,
        }
    }
}

/// 并发解析性能统计
#[derive(Debug, Clone)]
pub struct ConcurrentParseStats {
    pub decompress_workers: usize,
    pub parse_workers: usize,
    pub frame_workers: usize,
    pub batch_size: usize,
}

/// 并发解析器构建器
pub struct ConcurrentFrameParserBuilder {
    memory_pool: Option<Arc<MemoryPool>>,
    dbc_parser: Option<Arc<DbcParser>>,
    file_reader: Option<CachedFileReader>,
    config: ConcurrentParseConfig,
}

impl ConcurrentFrameParserBuilder {
    pub fn new() -> Self {
        Self {
            memory_pool: None,
            dbc_parser: None,
            file_reader: None,
            config: ConcurrentParseConfig::default(),
        }
    }

    pub fn memory_pool(mut self, pool: Arc<MemoryPool>) -> Self {
        self.memory_pool = Some(pool);
        self
    }

    pub fn dbc_parser(mut self, parser: Arc<DbcParser>) -> Self {
        self.dbc_parser = Some(parser);
        self
    }

    pub fn file_reader(mut self, reader: CachedFileReader) -> Self {
        self.file_reader = Some(reader);
        self
    }

    pub fn decompress_workers(mut self, count: usize) -> Self {
        self.config.decompress_workers = count;
        self
    }

    pub fn parse_workers(mut self, count: usize) -> Self {
        self.config.parse_workers = count;
        self
    }

    pub fn frame_workers(mut self, count: usize) -> Self {
        self.config.frame_workers = count;
        self
    }

    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    pub fn build(self) -> Result<ConcurrentFrameParser> {
        Ok(ConcurrentFrameParser::new(
            self.memory_pool.ok_or_else(|| anyhow!("Memory pool required"))?,
            self.dbc_parser.ok_or_else(|| anyhow!("DBC parser required"))?,
            self.file_reader,
            Some(self.config),
        ))
    }
}

impl Default for ConcurrentFrameParserBuilder {
    fn default() -> Self {
        Self::new()
    }
}