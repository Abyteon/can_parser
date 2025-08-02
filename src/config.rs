use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub input_dir: PathBuf,
    pub dbc_file: PathBuf,
    pub output_dir: PathBuf,
    pub workers: usize,
    pub batch_size: usize,
    pub memory_pool_size: usize,
    pub chunk_size: usize,
    pub max_file_size: usize,
    pub cache_max_entries: usize,
    pub cache_ttl_seconds: u64,
}

impl Config {
    pub fn new(
        input_dir: String,
        dbc_file: String,
        output_dir: String,
        workers: usize,
        batch_size: usize,
        memory_pool_size: usize,
    ) -> Self {
        Self {
            input_dir: PathBuf::from(input_dir),
            dbc_file: PathBuf::from(dbc_file),
            output_dir: PathBuf::from(output_dir),
            workers,
            batch_size,
            memory_pool_size,
            chunk_size: 1024 * 1024, // 1MB chunks
            max_file_size: 15 * 1024 * 1024, // 15MB
            cache_max_entries: 1000, // 最多1000个文件
            cache_ttl_seconds: 3600, // 1小时TTL
        }
    }

    pub fn memory_pool_size_bytes(&self) -> usize {
        self.memory_pool_size * 1024 * 1024
    }
} 