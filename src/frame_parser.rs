use anyhow::{Result, anyhow};
use byteorder::{BigEndian, ReadBytesExt};
use flate2::read::GzDecoder;
use std::io::{Cursor, Read};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::memory_pool::MemoryPool;
use crate::dbc_parser::DbcParser;
use crate::file_cache::CachedFileReader;

/// 第0层：文件级别的header和压缩数据
#[derive(Debug, Clone)]
pub struct FileLevelData {
    pub header: [u8; 35],
    pub data_length: u32,
    pub compressed_data: Vec<u8>,
}

/// 第1层：解压后的帧序列数据
#[derive(Debug, Clone)]
pub struct FrameSequenceData {
    pub header: [u8; 20],
    pub data_length: u32,
    pub frame_sequences: Vec<u8>,
}

/// 第2层：帧集合数据
#[derive(Debug, Clone)]
pub struct FramesData {
    pub header: [u8; 20],
    pub data_length: u32,
    pub frames: Vec<u8>,
}

/// 第3层：单个帧数据
#[derive(Debug, Clone)]
pub struct FrameData {
    pub header: [u8; 4],
    pub data_length: u32,
    pub frame_content: Vec<u8>,
}

/// 解析后的CAN帧
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedFrame {
    pub timestamp: u64,
    pub can_id: u32,
    pub data: Vec<u8>,
    pub signals: std::collections::HashMap<String, f64>,
}

/// 高性能帧解析器
pub struct FrameParser {
    memory_pool: Arc<MemoryPool>,
    dbc_parser: Arc<DbcParser>,
    file_reader: Option<CachedFileReader>,
}

impl FrameParser {
    pub fn new(memory_pool: Arc<MemoryPool>, dbc_parser: Arc<DbcParser>) -> Self {
        Self {
            memory_pool,
            dbc_parser,
            file_reader: None,
        }
    }

    pub fn with_file_reader(memory_pool: Arc<MemoryPool>, dbc_parser: Arc<DbcParser>, file_reader: CachedFileReader) -> Self {
        Self {
            memory_pool,
            dbc_parser,
            file_reader: Some(file_reader),
        }
    }

    /// 解析单个.bin文件
    pub async fn parse_file(&self, file_path: &str) -> Result<Vec<ParsedFrame>> {
        let mut buffer = self.memory_pool.get_buffer().await
            .unwrap_or_else(|| Vec::with_capacity(1024));
        
        // 使用缓存读取文件
        let file_data = if let Some(ref reader) = self.file_reader {
            if let Some(data) = reader.read_file(std::path::Path::new(file_path)).await? {
                data.to_vec()
            } else {
                return Err(anyhow!("文件不存在: {}", file_path));
            }
        } else {
            // 回退到直接文件读取
            tokio::fs::read(file_path).await?
        };
        
        buffer.extend_from_slice(&file_data);

        let mut frames = Vec::new();
        let mut offset = 0;

        // 解析第0层：文件级别的压缩数据块
        while offset < buffer.len() {
            let file_level = self.parse_file_level(&buffer[offset..])?;
            offset += 35 + file_level.data_length as usize;

            // 解压数据
            let decompressed = self.decompress_data(&file_level.compressed_data).await?;
            
            // 解析第1层：帧序列
            let frame_sequences = self.parse_frame_sequences(&decompressed).await?;
            
            // 解析第2层：帧集合
            for sequence in frame_sequences {
                let frames_data = self.parse_frames(&sequence).await?;
                
                // 解析第3层：单个帧
                for frame_data in frames_data {
                    if let Ok(parsed_frame) = self.parse_single_frame(&frame_data).await {
                        frames.push(parsed_frame);
                    }
                }
            }
        }

        // 归还buffer到池中
        self.memory_pool.return_buffer(buffer).await;
        Ok(frames)
    }

    /// 解析第0层：文件级别的header和压缩数据
    fn parse_file_level(&self, data: &[u8]) -> Result<FileLevelData> {
        if data.len() < 35 {
            return Err(anyhow!("数据长度不足，无法解析文件级别header"));
        }

        let mut cursor = Cursor::new(data);
        let mut header = [0u8; 35];
        cursor.read_exact(&mut header)?;

        // 读取数据长度（第31-35字节，大端序）
        let data_length = cursor.read_u32::<BigEndian>()?;

        if data.len() < 35 + data_length as usize {
            return Err(anyhow!("数据长度不足，无法读取压缩数据"));
        }

        let compressed_data = data[35..35 + data_length as usize].to_vec();

        Ok(FileLevelData {
            header,
            data_length,
            compressed_data,
        })
    }

    /// 解压gzip数据
    async fn decompress_data(&self, compressed_data: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = GzDecoder::new(compressed_data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }

    /// 解析第1层：帧序列数据
    async fn parse_frame_sequences(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        let mut sequences = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            if data.len() - offset < 20 {
                break;
            }

            let mut cursor = Cursor::new(&data[offset..]);
            let mut header = [0u8; 20];
            cursor.read_exact(&mut header)?;

            let data_length = cursor.read_u32::<BigEndian>()?;

            if data.len() - offset < 20 + data_length as usize {
                break;
            }

            let sequence_data = data[offset + 20..offset + 20 + data_length as usize].to_vec();
            sequences.push(sequence_data);
            offset += 20 + data_length as usize;
        }

        Ok(sequences)
    }

    /// 解析第2层：帧集合数据
    async fn parse_frames(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        let mut frames = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            if data.len() - offset < 20 {
                break;
            }

            let mut cursor = Cursor::new(&data[offset..]);
            let mut header = [0u8; 20];
            cursor.read_exact(&mut header)?;

            let data_length = cursor.read_u32::<BigEndian>()?;

            if data.len() - offset < 20 + data_length as usize {
                break;
            }

            let frame_data = data[offset + 20..offset + 20 + data_length as usize].to_vec();
            frames.push(frame_data);
            offset += 20 + data_length as usize;
        }

        Ok(frames)
    }

    /// 解析单个帧并转换为CAN帧
    async fn parse_single_frame(&self, data: &[u8]) -> Result<ParsedFrame> {
        let frame_data = self.parse_frame_data(data).await?;
        self.parse_can_frame(&frame_data).await
    }

    /// 解析帧数据结构
    async fn parse_frame_data(&self, data: &[u8]) -> Result<FrameData> {
        if data.len() < 4 {
            return Err(anyhow!("数据长度不足，无法解析帧header"));
        }

        let mut cursor = Cursor::new(data);
        let mut header = [0u8; 4];
        cursor.read_exact(&mut header)?;

        let data_length = cursor.read_u32::<BigEndian>()?;

        if data.len() < 4 + data_length as usize {
            return Err(anyhow!("数据长度不足，无法读取帧内容"));
        }

        let frame_content = data[4..4 + data_length as usize].to_vec();

        Ok(FrameData {
            header,
            data_length,
            frame_content,
        })
    }

    /// 解析CAN帧并提取信号
    pub async fn parse_can_frame(&self, frame_data: &FrameData) -> Result<ParsedFrame> {
        // 这里需要根据实际的CAN帧格式来解析
        // 假设前8字节是CAN ID，接下来是数据
        if frame_data.frame_content.len() < 8 {
            return Err(anyhow!("CAN帧数据长度不足"));
        }

        let can_id = u32::from_be_bytes([
            frame_data.frame_content[0],
            frame_data.frame_content[1],
            frame_data.frame_content[2],
            frame_data.frame_content[3],
        ]);

        let data = frame_data.frame_content[4..].to_vec();

        // 使用DBC解析器提取信号
        let signals = self.dbc_parser.parse_signals(can_id, &data).await?;

        Ok(ParsedFrame {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            can_id,
            data,
            signals,
        })
    }
} 