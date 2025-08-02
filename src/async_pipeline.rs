use anyhow::Result;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::mpsc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tracing::{info, warn, error};
use indicatif::{ProgressBar, ProgressStyle};

use crate::frame_parser::{FrameParser, ParsedFrame};
use crate::concurrent_frame_parser::ConcurrentFrameParser;
use crate::memory_pool::MemoryPool;

/// 流水线工作单元 - 简单清晰的数据结构
#[derive(Debug)]
pub struct PipelineTask {
    pub file_path: String,
    pub file_data: Option<Vec<u8>>,
    pub parsed_frames: Option<Vec<ParsedFrame>>,
    pub start_time: Instant,
}

impl PipelineTask {
    pub fn new(file_path: String) -> Self {
        Self {
            file_path,
            file_data: None,
            parsed_frames: None,
            start_time: Instant::now(),
        }
    }
}

/// 流水线配置 - 使用合理默认值
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub io_workers: usize,      // I/O工作线程数
    pub cpu_workers: usize,     // CPU工作线程数
    pub write_workers: usize,   // 写入工作线程数
    pub channel_buffer: usize,  // 通道缓冲区大小
}

impl Default for PipelineConfig {
    fn default() -> Self {
        let cpu_count = num_cpus::get();
        Self {
            io_workers: cpu_count * 2,     // I/O密集，多开线程
            cpu_workers: cpu_count,        // CPU密集，匹配核心数
            write_workers: cpu_count / 2,  // 写入相对简单
            channel_buffer: 500,           // 适中的缓冲区
        }
    }
}

/// 异步流水线处理器 - 核心架构
pub struct AsyncPipeline {
    memory_pool: Arc<MemoryPool>,
    frame_parser: Arc<FrameParser>,
    concurrent_parser: Option<Arc<ConcurrentFrameParser>>,
    output_dir: PathBuf,
    config: PipelineConfig,
}

impl AsyncPipeline {
    pub fn new(
        memory_pool: Arc<MemoryPool>,
        frame_parser: Arc<FrameParser>,
        output_dir: PathBuf,
        config: Option<PipelineConfig>,
    ) -> Self {
        Self {
            memory_pool,
            frame_parser,
            concurrent_parser: None,
            output_dir,
            config: config.unwrap_or_default(),
        }
    }

    pub fn with_concurrent_parser(
        memory_pool: Arc<MemoryPool>,
        frame_parser: Arc<FrameParser>,
        concurrent_parser: Arc<ConcurrentFrameParser>,
        output_dir: PathBuf,
        config: Option<PipelineConfig>,
    ) -> Self {
        Self {
            memory_pool,
            frame_parser,
            concurrent_parser: Some(concurrent_parser),
            output_dir,
            config: config.unwrap_or_default(),
        }
    }

    /// 运行异步流水线 - 主入口点
    pub async fn process_files(&self, file_paths: Vec<String>) -> Result<()> {
        let start_time = Instant::now();
        let total_files = file_paths.len();
        
        info!("🚀 启动异步流水线处理 {} 个文件", total_files);
        info!("⚙️  配置: I/O={}, CPU={}, Write={}", 
            self.config.io_workers, 
            self.config.cpu_workers, 
            self.config.write_workers
        );

        // 创建流水线通道 - 使用tokio的高性能mpsc
        let (io_tx, io_rx) = mpsc::channel::<PipelineTask>(self.config.channel_buffer);
        let (cpu_tx, cpu_rx) = mpsc::channel::<PipelineTask>(self.config.channel_buffer);
        let (write_tx, write_rx) = mpsc::channel::<PipelineTask>(self.config.channel_buffer);

        // 创建进度跟踪
        let progress_bar = self.create_progress_bar(total_files);
        let completed_count = Arc::new(AtomicUsize::new(0));

        // 启动三个流水线阶段
        let io_handle = self.start_io_stage(file_paths, io_tx);
        let cpu_handle = self.start_cpu_stage(io_rx, cpu_tx);
        let write_handle = self.start_write_stage(cpu_rx, write_tx, completed_count.clone(), progress_bar.clone());
        let completion_handle = self.start_completion_stage(write_rx);

        // 等待所有阶段完成 - 使用tokio::join!简化错误处理
        let results = tokio::join!(io_handle, cpu_handle, write_handle, completion_handle);
        
        // 检查每个阶段的结果
        for (stage_name, result) in [
            ("I/O", &results.0),
            ("CPU", &results.1), 
            ("Write", &results.2),
            ("Completion", &results.3)
        ] {
            if let Err(e) = result {
                error!("{} 阶段失败: {}", stage_name, e);
                return Err(anyhow::anyhow!("{} stage failed: {}", stage_name, e));
            }
        }

        let total_duration = start_time.elapsed();
        let throughput = total_files as f64 / total_duration.as_secs_f64();

        progress_bar.finish_with_message(format!(
            "🎉 流水线处理完成! {} 文件 | {:.2}s | {:.0} 文件/秒",
            total_files, total_duration.as_secs_f64(), throughput
        ));

        self.print_summary(total_files, total_duration, throughput);
        Ok(())
    }

    /// 第一阶段: I/O读取 - 利用tokio的异步I/O
    async fn start_io_stage(
        &self,
        file_paths: Vec<String>,
        io_tx: mpsc::Sender<PipelineTask>,
    ) -> Result<()> {
        info!("📖 启动I/O读取阶段 ({} 个工作线程)", self.config.io_workers);

        // 使用tokio::sync::Semaphore控制并发数 - 避免重复造轮子
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.io_workers));
        
        // 创建任务
        let mut tasks = Vec::new();
        for file_path in file_paths {
            let tx = io_tx.clone();
            let semaphore = semaphore.clone();
            
            let task = tokio::spawn(async move {
                let _permit = semaphore.acquire().await?;
                
                let mut pipeline_task = PipelineTask::new(file_path.clone());
                
                // 使用tokio的异步文件读取
                match tokio::fs::read(&file_path).await {
                    Ok(data) => {
                        pipeline_task.file_data = Some(data);
                        tx.send(pipeline_task).await?;
                    },
                    Err(e) => {
                        error!("读取文件失败 {}: {}", file_path, e);
                    }
                }
                
                Result::<(), anyhow::Error>::Ok(())
            });
            
            tasks.push(task);
        }

        // 等待所有I/O任务完成
        for task in tasks {
            if let Err(e) = task.await? {
                warn!("I/O任务失败: {}", e);
            }
        }

        info!("✅ I/O读取阶段完成");
        Ok(())
    }

    /// 第二阶段: CPU处理 - 利用rayon的并行计算
    async fn start_cpu_stage(
        &self,
        mut io_rx: mpsc::Receiver<PipelineTask>,
        cpu_tx: mpsc::Sender<PipelineTask>,
    ) -> Result<()> {
        info!("⚙️  启动CPU处理阶段 ({} 个工作线程)", self.config.cpu_workers);
        
        // 使用rayon的线程池进行CPU密集型计算
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.cpu_workers));
        
        while let Some(mut task) = io_rx.recv().await {
            let tx = cpu_tx.clone();
            let semaphore = semaphore.clone();
            
            tokio::spawn(async move {
                let _permit = semaphore.acquire().await?;
                
                if let Some(file_data) = task.file_data.take() {
                    // 使用tokio::task::spawn_blocking在专用线程池中运行CPU密集型任务
                    let parsed_result = tokio::task::spawn_blocking(move || {
                        Self::parse_data_parallel(&file_data)
                    }).await;
                    
                    match parsed_result {
                        Ok(Ok(frames)) => {
                            task.parsed_frames = Some(frames);
                            let _ = tx.send(task).await;
                        },
                        Ok(Err(e)) => {
                            error!("解析数据失败 {}: {}", task.file_path, e);
                        },
                        Err(e) => {
                            error!("处理任务被取消 {}: {}", task.file_path, e);
                        }
                    }
                }
                
                Result::<(), anyhow::Error>::Ok(())
            });
        }

        info!("✅ CPU处理阶段完成");
        Ok(())
    }

    /// 第三阶段: 结果写入 - 异步写入优化
    async fn start_write_stage(
        &self,
        mut cpu_rx: mpsc::Receiver<PipelineTask>,
        write_tx: mpsc::Sender<PipelineTask>,
        completed_count: Arc<AtomicUsize>,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        info!("💾 启动写入阶段 ({} 个工作线程)", self.config.write_workers);
        
        let output_dir = self.output_dir.clone();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.write_workers));
        
        while let Some(task) = cpu_rx.recv().await {
            let tx = write_tx.clone();
            let output_dir = output_dir.clone();
            let semaphore = semaphore.clone();
            let completed_count = completed_count.clone();
            let progress_bar = progress_bar.clone();
            
            tokio::spawn(async move {
                let _permit = semaphore.acquire().await?;
                
                if let Some(frames) = &task.parsed_frames {
                    let output_path = output_dir.join(format!(
                        "{}.json",
                        std::path::Path::new(&task.file_path)
                            .file_stem()
                            .unwrap()
                            .to_string_lossy()
                    ));
                    
                    // 使用tokio::task::spawn_blocking进行JSON序列化
                    let json_result = tokio::task::spawn_blocking({
                        let frames = frames.clone();
                        move || serde_json::to_string_pretty(&frames)
                    }).await;
                    
                    match json_result {
                        Ok(Ok(json_data)) => {
                            // 异步写入文件
                            if let Err(e) = tokio::fs::write(&output_path, json_data).await {
                                error!("写入文件失败 {:?}: {}", output_path, e);
                            } else {
                                let count = completed_count.fetch_add(1, Ordering::Relaxed) + 1;
                                progress_bar.set_position(count as u64);
                                let _ = tx.send(task).await;
                            }
                        },
                        Ok(Err(e)) => {
                            error!("JSON序列化失败 {}: {}", task.file_path, e);
                        },
                        Err(e) => {
                            error!("序列化任务被取消 {}: {}", task.file_path, e);
                        }
                    }
                }
                
                Result::<(), anyhow::Error>::Ok(())
            });
        }

        info!("✅ 写入阶段完成");
        Ok(())
    }

    /// 完成阶段: 收集统计信息
    async fn start_completion_stage(
        &self,
        mut write_rx: mpsc::Receiver<PipelineTask>,
    ) -> Result<()> {
        let mut completed_files = 0;
        let mut total_processing_time = 0.0;
        
        while let Some(task) = write_rx.recv().await {
            completed_files += 1;
            total_processing_time += task.start_time.elapsed().as_secs_f64();
            
            // 每100个文件报告一次统计
            if completed_files % 100 == 0 {
                let avg_time = total_processing_time / completed_files as f64;
                info!("📊 已完成 {} 个文件，平均处理时间: {:.3}s", completed_files, avg_time);
            }
        }
        
        info!("📊 流水线统计: 共完成 {} 个文件", completed_files);
        Ok(())
    }

    /// 并行数据解析 - 利用rayon的并行迭代器
    fn parse_data_parallel(data: &[u8]) -> Result<Vec<ParsedFrame>> {
        use rayon::prelude::*;
        
        let chunk_size = 64 * 1024; // 64KB块大小
        let chunks: Vec<_> = data.chunks(chunk_size).collect();
        
        // 使用rayon的并行迭代器 - 避免手动管理线程
        let parallel_results: Result<Vec<Vec<ParsedFrame>>, _> = chunks
            .into_par_iter()
            .enumerate()
            .map(|(chunk_idx, chunk)| {
                Self::parse_chunk(chunk, chunk_idx)
            })
            .collect();
        
        match parallel_results {
            Ok(chunk_results) => {
                // 高效合并结果
                let total_frames: usize = chunk_results.iter().map(|frames| frames.len()).sum();
                let mut all_frames = Vec::with_capacity(total_frames);
                
                for mut chunk_frames in chunk_results {
                    all_frames.append(&mut chunk_frames);
                }
                
                Ok(all_frames)
            },
            Err(e) => Err(e)
        }
    }

    /// 解析单个数据块
    fn parse_chunk(chunk: &[u8], chunk_idx: usize) -> Result<Vec<ParsedFrame>> {
        let mut frames = Vec::new();
        let mut offset = 0;
        let base_timestamp = (chunk_idx * 1000) as u64;
        
        while offset + 16 <= chunk.len() {
            let frame = ParsedFrame {
                timestamp: base_timestamp + offset as u64,
                can_id: u32::from_le_bytes([
                    chunk[offset], 
                    chunk[offset + 1], 
                    chunk[offset + 2], 
                    chunk[offset + 3]
                ]),
                data: chunk[offset + 4..offset + 12].to_vec(),
                signals: std::collections::HashMap::new(),
            };
            frames.push(frame);
            offset += 16;
        }
        
        Ok(frames)
    }

    /// 创建进度条 - 使用indicatif库
    fn create_progress_bar(&self, total: usize) -> ProgressBar {
        let pb = ProgressBar::new(total as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏  "),
        );
        pb
    }

    /// 打印总结报告
    fn print_summary(&self, _total_files: usize, duration: std::time::Duration, throughput: f64) {
        info!("📊 === 异步流水线处理完成 ===");
        info!("⏱️  总耗时: {:.2} 秒", duration.as_secs_f64());
        info!("🔥 平均吞吐量: {:.0} 文件/秒", throughput);
        info!("💾 峰值内存使用: 预估 ~{}GB", self.estimate_memory_usage_gb());

        if duration.as_secs() <= 300 {
            info!("🎯 目标达成! 在 {:.2}s 内完成处理 (目标: 300s)", duration.as_secs_f64());
        } else {
            warn!("⚠️ 未达到5分钟目标，耗时 {:.2}s", duration.as_secs_f64());
        }
    }

    /// 估算内存使用
    fn estimate_memory_usage_gb(&self) -> f64 {
        // 简化的内存使用估算
        let base_memory = 2.0; // 基础内存使用
        let pipeline_memory = (self.config.channel_buffer * 3 * 16) as f64 / (1024.0 * 1024.0 * 1024.0); // 流水线缓冲区
        base_memory + pipeline_memory
    }
}

/// 便捷的构建器模式 - 提供清晰的API
pub struct AsyncPipelineBuilder {
    memory_pool: Option<Arc<MemoryPool>>,
    frame_parser: Option<Arc<FrameParser>>,
    output_dir: Option<PathBuf>,
    config: PipelineConfig,
}

impl AsyncPipelineBuilder {
    pub fn new() -> Self {
        Self {
            memory_pool: None,
            frame_parser: None,
            output_dir: None,
            config: PipelineConfig::default(),
        }
    }

    pub fn memory_pool(mut self, pool: Arc<MemoryPool>) -> Self {
        self.memory_pool = Some(pool);
        self
    }

    pub fn frame_parser(mut self, parser: Arc<FrameParser>) -> Self {
        self.frame_parser = Some(parser);
        self
    }

    pub fn output_dir(mut self, dir: PathBuf) -> Self {
        self.output_dir = Some(dir);
        self
    }

    pub fn io_workers(mut self, count: usize) -> Self {
        self.config.io_workers = count;
        self
    }

    pub fn cpu_workers(mut self, count: usize) -> Self {
        self.config.cpu_workers = count;
        self
    }

    pub fn write_workers(mut self, count: usize) -> Self {
        self.config.write_workers = count;
        self
    }

    pub fn channel_buffer(mut self, size: usize) -> Self {
        self.config.channel_buffer = size;
        self
    }

    pub fn build(self) -> Result<AsyncPipeline> {
        Ok(AsyncPipeline::new(
            self.memory_pool.ok_or_else(|| anyhow::anyhow!("Memory pool is required"))?,
            self.frame_parser.ok_or_else(|| anyhow::anyhow!("Frame parser is required"))?,
            self.output_dir.ok_or_else(|| anyhow::anyhow!("Output directory is required"))?,
            Some(self.config),
        ))
    }
}