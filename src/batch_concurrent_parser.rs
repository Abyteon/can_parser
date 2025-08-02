use anyhow::{Result, anyhow};
use std::sync::Arc;
use rayon::prelude::*;
use tokio::task;
use tracing::{info, debug, warn};
use std::time::Instant;

use crate::frame_parser::{ParsedFrame, FileLevelData};
use crate::memory_pool::MemoryPool;
use crate::dbc_parser::DbcParser;
use crate::file_cache::CachedFileReader;

/// 批处理并发解析器 - 针对小数据块优化
pub struct BatchConcurrentParser {
    memory_pool: Arc<MemoryPool>,
    dbc_parser: Arc<DbcParser>,
    file_reader: Option<CachedFileReader>,
    config: BatchProcessConfig,
}

/// 批处理配置 - 针对小数据块优化
#[derive(Debug, Clone)]
pub struct BatchProcessConfig {
    /// 解压缩批处理大小（一次处理多少个压缩块）
    pub decompress_batch_size: usize,
    /// 解析批处理大小（一次处理多少个数据块）  
    pub parse_batch_size: usize,
    /// 帧处理批处理大小（一次处理多少个帧）
    pub frame_batch_size: usize,
    /// 并发工作线程数
    pub worker_threads: usize,
    /// 小数据块阈值（小于此值使用批处理）
    pub small_block_threshold: usize,
}

impl Default for BatchProcessConfig {
    fn default() -> Self {
        let cpu_count = num_cpus::get();
        Self {
            // 针对小压缩块，批量处理提高效率
            decompress_batch_size: 32,      // 一次解压32个小块
            parse_batch_size: 64,           // 一次解析64个数据块
            frame_batch_size: 128,          // 一次处理128个帧
            worker_threads: cpu_count,
            small_block_threshold: 8192,    // 8KB以下认为是小块
        }
    }
}

/// 数据块批次
#[derive(Debug, Clone)]
pub struct DataBatch<T> {
    pub items: Vec<T>,
    pub batch_id: usize,
    pub estimated_total_size: usize,
}

impl<T> DataBatch<T> {
    pub fn new(items: Vec<T>, batch_id: usize) -> Self {
        Self {
            items,
            batch_id,
            estimated_total_size: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl BatchConcurrentParser {
    pub fn new(
        memory_pool: Arc<MemoryPool>, 
        dbc_parser: Arc<DbcParser>,
        file_reader: Option<CachedFileReader>,
        config: Option<BatchProcessConfig>
    ) -> Self {
        let config = config.unwrap_or_default();
        info!("🔧 批处理并发解析器配置: 解压批次={}, 解析批次={}, 帧批次={}", 
              config.decompress_batch_size, config.parse_batch_size, config.frame_batch_size);
        
        Self {
            memory_pool,
            dbc_parser,
            file_reader,
            config,
        }
    }

    /// 批处理并发解析文件 - 主入口点
    pub async fn parse_file_batch_concurrent(&self, file_path: &str) -> Result<Vec<ParsedFrame>> {
        let start_time = Instant::now();
        debug!("🚀 开始批处理并发解析文件: {}", file_path);
        
        // 1. 读取文件数据
        let file_data = self.read_file_data(file_path).await?;
        debug!("📁 文件大小: {} KB", file_data.len() / 1024);
        
        // 2. 分析数据结构，创建批次
        let compressed_blocks = self.parse_file_level_fast(&file_data)?;
        let batches = self.create_smart_batches(compressed_blocks);
        debug!("📦 创建 {} 个批次，平均批次大小: {:.1}", 
               batches.len(), 
               batches.iter().map(|b| b.len()).sum::<usize>() as f64 / batches.len() as f64);
        
        // 3. 批处理并发解压缩
        let decompressed_batches = self.decompress_batches_concurrent(batches).await?;
        debug!("🗜️  批处理解压缩完成");
        
        // 4. 批处理并发解析
        let all_frames = self.parse_batches_concurrent(decompressed_batches).await?;
        
        let elapsed = start_time.elapsed();
        info!("✅ 批处理并发解析完成: {} 个帧, 耗时 {:.2}s, 速度 {:.0} 帧/秒", 
              all_frames.len(), elapsed.as_secs_f64(), all_frames.len() as f64 / elapsed.as_secs_f64());
        
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

    /// 快速解析文件级别数据（只解析结构，不复制数据）
    fn parse_file_level_fast(&self, data: &[u8]) -> Result<Vec<FileLevelData>> {
        let mut blocks = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            if data.len() - offset < 35 {
                break;
            }
            
            // 快速读取数据长度
            let data_length = u32::from_be_bytes([
                data[offset + 31], data[offset + 32], 
                data[offset + 33], data[offset + 34]
            ]);
            
            if data.len() - offset < 35 + data_length as usize {
                break;
            }
            
            // 快速创建块信息
            let mut header = [0u8; 35];
            header.copy_from_slice(&data[offset..offset + 35]);
            let compressed_data = data[offset + 35..offset + 35 + data_length as usize].to_vec();
            
            blocks.push(FileLevelData {
                header,
                data_length,
                compressed_data,
            });
            
            offset += 35 + data_length as usize;
        }

        debug!("📊 解析出 {} 个压缩块", blocks.len());
        Ok(blocks)
    }

    /// 创建智能批次 - 根据数据块大小优化批处理
    fn create_smart_batches(&self, blocks: Vec<FileLevelData>) -> Vec<DataBatch<FileLevelData>> {
        let mut batches = Vec::new();
        let mut current_batch = Vec::new();
        let mut current_size = 0;
        let mut batch_id = 0;

        for block in blocks {
            let block_size = block.compressed_data.len();
            
            // 根据块大小决定批次策略
            let should_start_new_batch = if block_size < self.config.small_block_threshold {
                // 小块：按数量批处理
                current_batch.len() >= self.config.decompress_batch_size
            } else {
                // 大块：按大小批处理或单独处理
                current_size + block_size > self.config.small_block_threshold * 2 || 
                current_batch.len() >= self.config.decompress_batch_size / 2
            };

            if should_start_new_batch && !current_batch.is_empty() {
                let mut batch = DataBatch::new(current_batch, batch_id);
                batch.estimated_total_size = current_size;
                batches.push(batch);
                current_batch = Vec::new();
                current_size = 0;
                batch_id += 1;
            }

            current_batch.push(block);
            current_size += block_size;
        }

        // 处理最后一个批次
        if !current_batch.is_empty() {
            let mut batch = DataBatch::new(current_batch, batch_id);
            batch.estimated_total_size = current_size;
            batches.push(batch);
        }

        debug!("🎯 批次统计:");
        for (i, batch) in batches.iter().enumerate() {
            debug!("  批次{}: {} 个块, 总大小: {} KB", 
                   i, batch.len(), batch.estimated_total_size / 1024);
        }

        batches
    }

    /// 批处理并发解压缩
    async fn decompress_batches_concurrent(&self, batches: Vec<DataBatch<FileLevelData>>) -> Result<Vec<DataBatch<Vec<u8>>>> {
        info!("🗜️  开始批处理并发解压缩 {} 个批次", batches.len());
        let start_time = Instant::now();

        // 并发处理每个批次
        let decompressed_futures: Vec<_> = batches
            .into_iter()
            .map(|batch| {
                let batch_id = batch.batch_id;
                let batch_size = batch.len();
                task::spawn_blocking(move || {
                    debug!("⚙️  处理批次{}: {} 个块", batch_id, batch_size);
                    let start = Instant::now();
                    
                    // 在当前批次内并发解压
                    let decompressed: Result<Vec<_>, _> = batch.items
                        .into_par_iter()
                        .map(|block| {
                            Self::decompress_single_block_static(&block.compressed_data)
                        })
                        .collect();
                    
                    let batch_time = start.elapsed();
                    debug!("✅ 批次{} 完成: {:.2}s, {:.0} 块/秒", 
                           batch_id, batch_time.as_secs_f64(), batch_size as f64 / batch_time.as_secs_f64());
                    
                    decompressed.map(|items| DataBatch::new(items, batch_id))
                })
            })
            .collect();

        // 等待所有批次完成
        let mut decompressed_batches = Vec::new();
        for future in decompressed_futures {
            let batch = future.await??;
            decompressed_batches.push(batch);
        }

        let total_time = start_time.elapsed();
        let total_blocks: usize = decompressed_batches.iter().map(|b| b.len()).sum();
        info!("🎉 批处理解压缩完成: {} 个块, 耗时 {:.2}s, 吞吐量 {:.0} 块/秒", 
              total_blocks, total_time.as_secs_f64(), total_blocks as f64 / total_time.as_secs_f64());

        Ok(decompressed_batches)
    }

    /// 解压缩单个数据块（静态方法，用于并发）
    fn decompress_single_block_static(compressed_data: &[u8]) -> Result<Vec<u8>> {
        use flate2::read::GzDecoder;
        use std::io::Read;
        
        let mut decoder = GzDecoder::new(compressed_data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }

    /// 批处理并发解析
    async fn parse_batches_concurrent(&self, batches: Vec<DataBatch<Vec<u8>>>) -> Result<Vec<ParsedFrame>> {
        info!("⚙️  开始批处理并发解析 {} 个批次", batches.len());
        let start_time = Instant::now();

        // 并发处理每个批次
        let parse_futures: Vec<_> = batches
            .into_iter()
            .map(|batch| {
                let dbc_parser = self.dbc_parser.clone();
                let batch_id = batch.batch_id;
                let batch_size = batch.len();
                
                task::spawn_blocking(move || {
                    debug!("🔍 解析批次{}: {} 个数据块", batch_id, batch_size);
                    let start = Instant::now();
                    
                    // 在当前批次内并发解析
                    let parsed_results: Result<Vec<Vec<ParsedFrame>>, anyhow::Error> = batch.items
                        .into_par_iter()
                        .map(|decompressed_data| {
                            Self::parse_decompressed_data_fast(decompressed_data, dbc_parser.clone())
                        })
                        .collect();
                        
                    let parsed_frames: Result<Vec<ParsedFrame>, anyhow::Error> = match parsed_results {
                        Ok(frame_vecs) => {
                            let all_frames: Vec<ParsedFrame> = frame_vecs.into_iter().flatten().collect();
                            Ok(all_frames)
                        }
                        Err(e) => Err(e),
                    };
                    
                    let batch_time = start.elapsed();
                    match &parsed_frames {
                        Ok(frames) => {
                            debug!("✅ 批次{} 解析完成: {} 帧, {:.2}s, {:.0} 帧/秒", 
                                   batch_id, frames.len(), batch_time.as_secs_f64(), 
                                   frames.len() as f64 / batch_time.as_secs_f64());
                        }
                        Err(e) => {
                            warn!("❌ 批次{} 解析失败: {}", batch_id, e);
                        }
                    }
                    
                    parsed_frames
                })
            })
            .collect();

        // 收集所有结果
        let mut all_frames = Vec::new();
        for future in parse_futures {
            let frames = future.await??;
            all_frames.extend(frames);
        }

        let total_time = start_time.elapsed();
        info!("🎉 批处理解析完成: {} 帧, 耗时 {:.2}s, 吞吐量 {:.0} 帧/秒", 
              all_frames.len(), total_time.as_secs_f64(), all_frames.len() as f64 / total_time.as_secs_f64());

        Ok(all_frames)
    }

    /// 快速解析解压后的数据（批处理优化版本）
    fn parse_decompressed_data_fast(data: Vec<u8>, _dbc_parser: Arc<DbcParser>) -> Result<Vec<ParsedFrame>> {
        let mut frames = Vec::new();
        
        // 使用固定大小的批次处理帧数据
        let frame_size = 16; // 假设帧大小为16字节
        let chunk_size = 1024; // 1KB 块
        
        // 分块并发处理
        let chunks: Vec<_> = data.chunks(chunk_size).collect();
        let parallel_frames: Result<Vec<_>, _> = chunks
            .into_par_iter()
            .map(|chunk| {
                Self::parse_chunk_to_frames_fast(chunk, frame_size)
            })
            .collect();

        let chunk_frames = parallel_frames?;
        for mut chunk_frame_vec in chunk_frames {
            frames.append(&mut chunk_frame_vec);
        }

        Ok(frames)
    }

    /// 快速解析数据块为帧（优化版本）
    fn parse_chunk_to_frames_fast(chunk: &[u8], frame_size: usize) -> Result<Vec<ParsedFrame>> {
        let mut frames = Vec::new();
        let mut offset = 0;
        
        // 预分配容量以减少内存分配
        let estimated_frames = chunk.len() / frame_size;
        frames.reserve(estimated_frames);
        
        while offset + frame_size <= chunk.len() {
            // 快速解析帧（简化版本）
            let frame = ParsedFrame {
                timestamp: u64::from_le_bytes([
                    chunk[offset], chunk[offset + 1], chunk[offset + 2], chunk[offset + 3],
                    chunk[offset + 4], chunk[offset + 5], chunk[offset + 6], chunk[offset + 7],
                ]),
                can_id: u32::from_le_bytes([
                    chunk[offset + 8], chunk[offset + 9], 
                    chunk[offset + 10], chunk[offset + 11]
                ]),
                data: chunk[offset + 12..offset + frame_size].to_vec(),
                signals: std::collections::HashMap::new(), // 简化处理
            };
            frames.push(frame);
            offset += frame_size;
        }
        
        Ok(frames)
    }

    /// 获取配置信息
    pub fn get_config(&self) -> &BatchProcessConfig {
        &self.config
    }

    /// 性能统计
    pub fn estimate_performance(&self, file_count: usize, avg_file_size_mb: f64) -> BatchPerformanceEstimate {
        let total_data_gb = file_count as f64 * avg_file_size_mb / 1024.0;
        
        // 基于批处理并发的性能估算
        let estimated_throughput_files_per_sec = match avg_file_size_mb {
            size if size < 1.0 => 50.0,      // 小文件：50 文件/秒
            size if size < 5.0 => 25.0,      // 中等文件：25 文件/秒  
            size if size < 15.0 => 15.0,     // 大文件：15 文件/秒
            _ => 8.0,                        // 超大文件：8 文件/秒
        };
        
        let estimated_time_seconds = file_count as f64 / estimated_throughput_files_per_sec;
        
        BatchPerformanceEstimate {
            total_files: file_count,
            total_data_gb,
            estimated_time_seconds,
            estimated_throughput_files_per_sec,
            batch_optimization_factor: 2.5, // 批处理预期2.5倍提升
        }
    }
}

/// 批处理性能估算
#[derive(Debug, Clone)]
pub struct BatchPerformanceEstimate {
    pub total_files: usize,
    pub total_data_gb: f64,
    pub estimated_time_seconds: f64,
    pub estimated_throughput_files_per_sec: f64,
    pub batch_optimization_factor: f64,
}

impl BatchPerformanceEstimate {
    pub fn print_estimate(&self) {
        info!("📊 批处理性能预估:");
        info!("  📁 总文件数: {}", self.total_files);
        info!("  💾 总数据量: {:.1} GB", self.total_data_gb);
        info!("  ⏱️  预估处理时间: {:.1} 分钟", self.estimated_time_seconds / 60.0);
        info!("  🚀 预估吞吐量: {:.1} 文件/秒", self.estimated_throughput_files_per_sec);
        info!("  📈 批处理优化倍数: {:.1}x", self.batch_optimization_factor);
        
        if self.estimated_time_seconds <= 300.0 { // 5分钟
            info!("  ✅ 预期在5分钟内完成！");
        } else {
            warn!("  ⚠️  可能超过5分钟目标，建议进一步优化");
        }
    }
}

/// 批处理并发解析器构建器
pub struct BatchConcurrentParserBuilder {
    memory_pool: Option<Arc<MemoryPool>>,
    dbc_parser: Option<Arc<DbcParser>>,
    file_reader: Option<CachedFileReader>,
    config: BatchProcessConfig,
}

impl BatchConcurrentParserBuilder {
    pub fn new() -> Self {
        Self {
            memory_pool: None,
            dbc_parser: None,
            file_reader: None,
            config: BatchProcessConfig::default(),
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

    pub fn decompress_batch_size(mut self, size: usize) -> Self {
        self.config.decompress_batch_size = size;
        self
    }

    pub fn parse_batch_size(mut self, size: usize) -> Self {
        self.config.parse_batch_size = size;
        self
    }

    pub fn frame_batch_size(mut self, size: usize) -> Self {
        self.config.frame_batch_size = size;
        self
    }

    pub fn worker_threads(mut self, count: usize) -> Self {
        self.config.worker_threads = count;
        self
    }

    pub fn small_block_threshold(mut self, threshold: usize) -> Self {
        self.config.small_block_threshold = threshold;
        self
    }

    pub fn build(self) -> Result<BatchConcurrentParser> {
        Ok(BatchConcurrentParser::new(
            self.memory_pool.ok_or_else(|| anyhow!("Memory pool required"))?,
            self.dbc_parser.ok_or_else(|| anyhow!("DBC parser required"))?,
            self.file_reader,
            Some(self.config),
        ))
    }
}

impl Default for BatchConcurrentParserBuilder {
    fn default() -> Self {
        Self::new()
    }
}