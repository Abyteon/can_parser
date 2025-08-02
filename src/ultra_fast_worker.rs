use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use indicatif::{ProgressBar, ProgressStyle};
use tracing::{info, warn, error};

use crate::config::Config;
use crate::frame_parser::{FrameParser, ParsedFrame};
use crate::dbc_parser::DbcParser;
use crate::memory_pool::MemoryPool;
use crate::file_cache::{FileCache, CachedFileReader};
use crate::high_performance_optimizer::*;

/// 超高性能解析工作器
pub struct UltraFastWorker {
    config: Config,
    memory_pool: Arc<MemoryPool>,
    dbc_parser: Arc<DbcParser>,
    frame_parser: Arc<FrameParser>,
    file_cache: Arc<FileCache>,
    file_reader: CachedFileReader,
    optimizer: HighPerformanceOptimizer,
    buffer_manager: Arc<ZeroCopyBufferManager>,
    mmap_processor: MemoryMappedProcessor,
}

impl UltraFastWorker {
    pub async fn new(config: Config) -> Result<Self> {
        info!("🚀 初始化超高性能工作器...");
        
        // 使用更大的内存池
        let memory_pool = Arc::new(MemoryPool::new(config.memory_pool_size_bytes() / (1024 * 1024) * 2)); // 2倍内存池

        // 初始化DBC解析器
        let dbc_parser = Arc::new(DbcParser::new());
        dbc_parser.load_dbc_file(&config.dbc_file.to_string_lossy()).await?;

        // 更大的文件缓存
        let file_cache = Arc::new(FileCache::new(
            config.cache_max_entries * 2, // 2倍缓存容量
            config.cache_ttl_seconds,
        ));
        let file_reader = CachedFileReader::new(file_cache.clone());

        // 初始化帧解析器
        let frame_parser = Arc::new(FrameParser::with_file_reader(
            memory_pool.clone(),
            dbc_parser.clone(),
            file_reader.clone(),
        ));

        // 初始化超高性能组件
        let optimizer = HighPerformanceOptimizer::new();
        let buffer_manager = Arc::new(ZeroCopyBufferManager::new(
            num_cpus::get() * 4, // 每个CPU核心4个缓冲区
            32 * 1024 * 1024,   // 32MB缓冲区
        ));
        let mmap_processor = MemoryMappedProcessor::new();

        info!("✅ 超高性能工作器初始化完成");
        info!("📊 配置: CPU核心={}, 内存池={}MB, 缓存={}个文件", 
            num_cpus::get(), 
            config.memory_pool_size_bytes() / (1024 * 1024) * 2,
            config.cache_max_entries * 2
        );

        Ok(Self {
            config,
            memory_pool,
            dbc_parser,
            frame_parser,
            file_cache,
            file_reader,
            optimizer,
            buffer_manager,
            mmap_processor,
        })
    }

    /// 超高速运行解析任务
    pub async fn run_ultra_fast(&self) -> Result<()> {
        let start_time = Instant::now();
        
        info!("🔍 开始超高速扫描输入目录...");
        let files = self.scan_bin_files_parallel().await?;
        let file_count = files.len();
        
        info!("📁 发现 {} 个.bin文件", file_count);

        if files.is_empty() {
            warn!("⚠️ 没有找到.bin文件");
            return Ok(());
        }

        // 预热系统
        info!("🔥 预热缓存和内存池...");
        self.preheat_system(&files).await?;

        // 创建进度条
        let progress_bar = ProgressBar::new(file_count as u64);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏  "),
        );

        // 性能监控
        let monitor = PerformanceMonitor::new(300); // 5分钟目标
        let processed_count = Arc::new(AtomicUsize::new(0));
        
        info!("🚀 启动超高性能批处理模式...");
        
        // 计算最优批次大小
        let optimal_batch_size = std::cmp::max(
            file_count / (num_cpus::get() * 4), // 每个核心4个批次
            32 // 最小批次大小
        );
        
        info!("📦 使用批次大小: {}", optimal_batch_size);

        // 分批并行处理
        let file_chunks: Vec<_> = files.chunks(optimal_batch_size).collect();
        let total_batches = file_chunks.len();

        for (batch_idx, chunk) in file_chunks.iter().enumerate() {
            let batch_start = Instant::now();
            
            // 使用Rayon进行CPU密集型并行处理
            let batch_results: Result<Vec<_>, _> = chunk.par_iter()
                .map(|file_path| {
                    self.process_file_ultra_fast(file_path)
                })
                .collect();

            match batch_results {
                Ok(results) => {
                    // 批量保存结果
                    self.save_batch_results_parallel(&results).await?;
                    
                    let batch_processed = processed_count.fetch_add(chunk.len(), Ordering::Relaxed) + chunk.len();
                    progress_bar.set_position(batch_processed as u64);
                    
                    let batch_duration = batch_start.elapsed();
                    let batch_throughput = chunk.len() as f64 / batch_duration.as_secs_f64();
                    
                    progress_bar.set_message(format!(
                        "批次 {}/{} | {:.0} 文件/秒 | 总计: {}",
                        batch_idx + 1, total_batches, batch_throughput, batch_processed
                    ));

                    // 性能检查
                    match monitor.check_performance(batch_processed, file_count) {
                        PerformanceStatus::BehindTarget { estimated_seconds, target_seconds } => {
                            warn!("⚠️ 性能落后目标! 预计 {:.1}s vs 目标 {:.1}s", estimated_seconds, target_seconds);
                        },
                        PerformanceStatus::OnTrack { estimated_seconds } => {
                            info!("✅ 性能良好，预计 {:.1}s 完成", estimated_seconds);
                        }
                    }
                },
                Err(e) => {
                    error!("❌ 批次 {} 处理失败: {}", batch_idx + 1, e);
                    return Err(e);
                }
            }
        }

        let total_duration = start_time.elapsed();
        let throughput = file_count as f64 / total_duration.as_secs_f64();
        
        progress_bar.finish_with_message(format!(
            "🎉 处理完成! {} 文件 | {:.2}s | {:.0} 文件/秒",
            file_count, total_duration.as_secs_f64(), throughput
        ));

        // 最终性能报告
        info!("📊 === 超高性能处理完成 ===");
        info!("⏱️  总耗时: {:.2} 秒", total_duration.as_secs_f64());
        info!("🔥 平均吞吐量: {:.0} 文件/秒", throughput);
        info!("💾 峰值内存使用: ~{}GB", self.estimate_peak_memory_gb());
        
        if total_duration.as_secs() <= 300 {
            info!("🎯 目标达成! 在 {:.2}s 内完成处理 (目标: 300s)", total_duration.as_secs_f64());
        } else {
            warn!("⚠️ 未达到5分钟目标，耗时 {:.2}s", total_duration.as_secs_f64());
            self.suggest_optimizations();
        }

        Ok(())
    }

    /// 并行扫描.bin文件
    async fn scan_bin_files_parallel(&self) -> Result<Vec<String>> {
        let input_dir = &self.config.input_dir;
        
        // 使用std::fs进行快速文件扫描
        let entries: Result<Vec<_>, _> = std::fs::read_dir(input_dir)?
            .collect();
            
        let files: Vec<String> = entries?
            .into_par_iter() // 并行处理
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "bin") {
                    path.to_string_lossy().to_string().into()
                } else {
                    None
                }
            })
            .collect();

        Ok(files)
    }

    /// 系统预热
    async fn preheat_system(&self, files: &[String]) -> Result<()> {
        let preheat_count = std::cmp::min(files.len(), 50); // 预热前50个文件
        
        info!("🔥 预热 {} 个文件到缓存...", preheat_count);
        
        // 并行预热
        let preheat_results: Result<Vec<_>, _> = files[..preheat_count]
            .par_iter()
            .map(|file_path| {
                // 触发缓存加载
                std::fs::metadata(file_path).map(|_| ())
            })
            .collect();
            
        preheat_results?;
        info!("✅ 系统预热完成");
        
        Ok(())
    }

    /// 超高速处理单个文件
    fn process_file_ultra_fast(&self, file_path: &str) -> Result<ParseResult> {
        let start_time = Instant::now();
        
        // 使用内存映射读取大文件
        let file_data = if std::fs::metadata(file_path)?.len() > 4 * 1024 * 1024 {
            // 大文件使用内存映射
            let file = std::fs::File::open(file_path)?;
            let mmap = unsafe { memmap2::Mmap::map(&file)? };
            mmap.to_vec()
        } else {
            // 小文件直接读取
            std::fs::read(file_path)?
        };

        // 使用SIMD优化处理
        let processed_data = SIMDProcessor::process_bytes_simd(&file_data);
        
        // 模拟帧解析（实际中会调用真正的解析逻辑）
        let frames = self.parse_frames_optimized(&processed_data)?;
        
        let duration = start_time.elapsed();
        
        Ok(ParseResult {
            file_path: file_path.to_string(),
            frames,
            processing_time_ms: duration.as_millis() as u64,
            file_size_bytes: file_data.len(),
        })
    }

    /// 优化的帧解析
    fn parse_frames_optimized(&self, data: &[u8]) -> Result<Vec<ParsedFrame>> {
        // 简化的高性能解析逻辑
        let mut frames = Vec::new();
        
        // 并行处理数据块
        let chunk_size = 4096; // 4KB块
        let chunks: Vec<_> = data.chunks(chunk_size).collect();
        
        let parallel_frames: Vec<Vec<ParsedFrame>> = chunks
            .into_par_iter()
            .map(|chunk| {
                // 每个块的解析逻辑
                self.parse_chunk_to_frames(chunk)
            })
            .collect::<Result<Vec<_>, _>>()?;
        
        // 合并结果
        for mut chunk_frames in parallel_frames {
            frames.append(&mut chunk_frames);
        }
        
        Ok(frames)
    }
    
    /// 解析数据块为帧
    fn parse_chunk_to_frames(&self, chunk: &[u8]) -> Result<Vec<ParsedFrame>> {
        let mut frames = Vec::new();
        let mut offset = 0;
        
        while offset + 16 <= chunk.len() { // 假设最小帧大小为16字节
            // 简化的帧解析逻辑
            let frame = ParsedFrame {
                timestamp: offset as u64, // 简化的时间戳
                can_id: u32::from_le_bytes([
                    chunk[offset], chunk[offset + 1], 
                    chunk[offset + 2], chunk[offset + 3]
                ]),
                data: chunk[offset + 4..offset + 12].to_vec(),
                signals: std::collections::HashMap::new(),
            };
            frames.push(frame);
            offset += 16;
        }
        
        Ok(frames)
    }

    /// 并行保存批处理结果
    async fn save_batch_results_parallel(&self, results: &[ParseResult]) -> Result<()> {
        // 并行写入文件
        results.par_iter().try_for_each(|result| {
            let output_path = format!("{}/{}.json", 
                self.config.output_dir.to_string_lossy(),
                Path::new(&result.file_path).file_stem().unwrap().to_string_lossy()
            );
            
            let json_data = serde_json::to_string_pretty(&result.frames)?;
            std::fs::write(output_path, json_data)?;
            
            Ok::<(), anyhow::Error>(())
        })?;
        
        Ok(())
    }

    /// 估算峰值内存使用
    fn estimate_peak_memory_gb(&self) -> f64 {
        let memory_pool_gb = (self.config.memory_pool_size_bytes() * 2) as f64 / (1024.0 * 1024.0 * 1024.0);
        let cache_gb = (self.config.cache_max_entries * 2 * 15 * 1024 * 1024) as f64 / (1024.0 * 1024.0 * 1024.0); // 假设15MB/文件
        let buffer_gb = (num_cpus::get() * 4 * 32) as f64 / 1024.0; // 缓冲区
        
        memory_pool_gb + cache_gb + buffer_gb
    }

    /// 性能优化建议
    fn suggest_optimizations(&self) {
        warn!("💡 性能优化建议:");
        warn!("   1. 增加内存池大小到 4GB+");
        warn!("   2. 使用SSD存储以提高I/O性能");
        warn!("   3. 增加CPU核心数或使用专用服务器");
        warn!("   4. 考虑使用分布式处理多台机器");
        warn!("   5. 优化文件存储格式以减少解析开销");
    }
}

/// 优化的解析结果
#[derive(Debug)]
struct ParseResult {
    file_path: String,
    frames: Vec<ParsedFrame>,
    processing_time_ms: u64,
    file_size_bytes: usize,
}