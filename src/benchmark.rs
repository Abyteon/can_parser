use std::time::Instant;
use anyhow::Result;
use tracing::info;

use crate::memory_pool::MemoryPool;
use crate::file_cache::FileCache;
use crate::config::Config;

/// 性能测试结果
#[derive(Debug)]
pub struct BenchmarkResult {
    pub test_name: String,
    pub duration_ms: u64,
    pub throughput_mb_s: f64,
    pub memory_usage_mb: usize,
}

/// 性能测试套件
pub struct Benchmark {
    config: Config,
    memory_pool: MemoryPool,
    file_cache: FileCache,
}

impl Benchmark {
    pub fn new(config: Config) -> Self {
        let memory_pool = MemoryPool::new(config.memory_pool_size);
        let file_cache = FileCache::new(
            config.cache_max_entries,
            config.cache_ttl_seconds,
        );

        Self {
            config,
            memory_pool,
            file_cache,
        }
    }

    /// 运行所有性能测试
    pub async fn run_all(&self) -> Result<Vec<BenchmarkResult>> {
        let mut results = Vec::new();

        info!("开始性能测试...");

        // 内存池性能测试
        results.push(self.benchmark_memory_pool().await?);

        // 文件缓存性能测试
        results.push(self.benchmark_file_cache().await?);

        // 内存分配性能测试
        results.push(self.benchmark_memory_allocation().await?);

        // 文件读取性能测试
        results.push(self.benchmark_file_reading().await?);

        self.print_results(&results);
        Ok(results)
    }

    /// 内存池性能测试
    async fn benchmark_memory_pool(&self) -> Result<BenchmarkResult> {
        let start = Instant::now();
        let iterations = 10000;

        for _ in 0..iterations {
            if let Some(buffer) = self.memory_pool.get_buffer().await {
                self.memory_pool.return_buffer(buffer).await;
            }
        }

        let duration = start.elapsed();
        let stats = self.memory_pool.stats().await;

        Ok(BenchmarkResult {
            test_name: "内存池操作".to_string(),
            duration_ms: duration.as_millis() as u64,
            throughput_mb_s: (iterations as f64) / (duration.as_secs_f64() * 1024.0 * 1024.0),
            memory_usage_mb: stats.current_usage / (1024 * 1024),
        })
    }

    /// 文件缓存性能测试
    async fn benchmark_file_cache(&self) -> Result<BenchmarkResult> {
        let start = Instant::now();
        let test_file = "test_example.dbc";
        let iterations = 1000;

        for _ in 0..iterations {
            let _ = self.file_cache.get(std::path::Path::new(test_file)).await;
        }

        let duration = start.elapsed();
        let stats = self.file_cache.stats().await;

        Ok(BenchmarkResult {
            test_name: "文件缓存操作".to_string(),
            duration_ms: duration.as_millis() as u64,
            throughput_mb_s: (iterations as f64) / (duration.as_secs_f64() * 1024.0 * 1024.0),
            memory_usage_mb: stats.total_size / (1024 * 1024),
        })
    }

    /// 内存分配性能测试
    async fn benchmark_memory_allocation(&self) -> Result<BenchmarkResult> {
        let start = Instant::now();
        let iterations = 10000;

        for _ in 0..iterations {
            if let Some(buffer) = self.memory_pool.get_buffer().await {
                self.memory_pool.return_buffer(buffer).await;
            }
        }

        let duration = start.elapsed();

        Ok(BenchmarkResult {
            test_name: "内存块分配".to_string(),
            duration_ms: duration.as_millis() as u64,
            throughput_mb_s: (iterations as f64) / (duration.as_secs_f64() * 1024.0 * 1024.0),
            memory_usage_mb: 0, // 动态计算
        })
    }

    /// 文件读取性能测试
    async fn benchmark_file_reading(&self) -> Result<BenchmarkResult> {
        let start = Instant::now();
        let test_file = "test_example.dbc";
        let iterations = 100;

        for _ in 0..iterations {
            let _ = tokio::fs::read(test_file).await;
        }

        let duration = start.elapsed();
        let file_size = std::fs::metadata(test_file)?.len() as usize;

        Ok(BenchmarkResult {
            test_name: "文件读取".to_string(),
            duration_ms: duration.as_millis() as u64,
            throughput_mb_s: (iterations as f64 * file_size as f64) / (duration.as_secs_f64() * 1024.0 * 1024.0),
            memory_usage_mb: 0,
        })
    }

    /// 打印测试结果
    fn print_results(&self, results: &[BenchmarkResult]) {
        info!("=== 性能测试结果 ===");
        
        for result in results {
            info!(
                "{}: {}ms, {:.2} MB/s, 内存使用: {}MB",
                result.test_name,
                result.duration_ms,
                result.throughput_mb_s,
                result.memory_usage_mb
            );
        }

        info!("=== 测试完成 ===");
    }

    /// 生成性能报告
    pub fn generate_report(&self, results: &[BenchmarkResult]) -> String {
        let mut report = String::new();
        report.push_str("# CAN Parser 性能测试报告\n\n");

        for result in results {
            report.push_str(&format!(
                "## {}\n",
                result.test_name
            ));
            report.push_str(&format!(
                "- 耗时: {}ms\n",
                result.duration_ms
            ));
            report.push_str(&format!(
                "- 吞吐量: {:.2} MB/s\n",
                result.throughput_mb_s
            ));
            report.push_str(&format!(
                "- 内存使用: {}MB\n\n",
                result.memory_usage_mb
            ));
        }

        report
    }
} 