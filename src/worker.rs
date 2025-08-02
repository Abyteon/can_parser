use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{broadcast, Semaphore};
use tokio::task::JoinSet;
use tracing::{info, warn, error};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::VecDeque;

use crate::config::Config;
use crate::frame_parser::{FrameParser, ParsedFrame};
use crate::dbc_parser::DbcParser;
use crate::memory_pool::MemoryPool;
use crate::file_cache::{FileCache, CachedFileReader};


/// 文件处理任务
#[derive(Debug, Clone)]
struct FileTask {
    file_path: String,
    file_index: usize,
}

/// 解析结果
#[derive(Debug, Clone)]
struct ParseResult {
    file_path: String,
    file_index: usize,
    frames: Vec<ParsedFrame>,
    error: Option<String>,
}

/// 高性能解析工作器
pub struct ParserWorker {
    config: Config,
    memory_pool: Arc<MemoryPool>,
    dbc_parser: Arc<DbcParser>,
    frame_parser: Arc<FrameParser>,
    file_cache: Arc<FileCache>,
    file_reader: CachedFileReader,
}

impl ParserWorker {
    pub async fn new(config: Config) -> Result<Self> {
        // 初始化内存池
        let memory_pool = Arc::new(MemoryPool::new(config.memory_pool_size_bytes() / (1024 * 1024)));

        // 初始化DBC解析器
        let dbc_parser = Arc::new(DbcParser::new());
        dbc_parser.load_dbc_file(&config.dbc_file.to_string_lossy()).await?;

        // 初始化文件缓存
        let file_cache = Arc::new(FileCache::new(
            config.cache_max_entries,
            config.cache_ttl_seconds,
        ));
        let file_reader = CachedFileReader::new(file_cache.clone());

        // 初始化帧解析器
        let frame_parser = Arc::new(FrameParser::with_file_reader(
            memory_pool.clone(),
            dbc_parser.clone(),
            file_reader.clone(),
        ));

        Ok(Self {
            config,
            memory_pool,
            dbc_parser,
            frame_parser,
            file_cache,
            file_reader,
        })
    }

    /// 运行解析任务
    pub async fn run(&self) -> Result<()> {
        info!("开始扫描输入目录...");
        
        // 扫描所有.bin文件
        let files = self.scan_bin_files().await?;
        info!("找到 {} 个.bin文件", files.len());

        // 预热文件缓存
        info!("预热文件缓存...");
        self.file_cache.warmup(&self.config.input_dir).await?;
        let cache_stats = self.file_cache.stats().await;
        info!("缓存预热完成，已加载 {} 个文件", cache_stats.cache_size);

        if files.is_empty() {
            warn!("没有找到.bin文件");
            return Ok(());
        }

        // 创建进度条
        let progress_bar = ProgressBar::new(files.len() as u64);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );

        // 创建任务通道
        let (task_tx, _) = broadcast::channel::<FileTask>(self.config.workers * 2);
        let (result_tx, mut result_rx) = broadcast::channel::<ParseResult>(self.config.workers * 2);

        // 启动工作线程
        let mut join_set = JoinSet::new();
        let semaphore = Arc::new(Semaphore::new(self.config.workers));

        // 启动工作线程
        for worker_id in 0..self.config.workers {
            let mut task_rx = task_tx.subscribe();
            let result_tx = result_tx.clone();
            let semaphore = semaphore.clone();
            let frame_parser = self.frame_parser.clone();
            let memory_pool = self.memory_pool.clone();
            let batch_size = self.config.batch_size;

            join_set.spawn(async move {
                Self::worker_loop(
                    worker_id,
                    task_rx,
                    result_tx,
                    semaphore,
                    frame_parser,
                    memory_pool,
                    batch_size,
                ).await;
            });
        }

        // 发送任务
        for (index, file_path) in files.into_iter().enumerate() {
            let task = FileTask {
                file_path,
                file_index: index,
            };
            task_tx.send(task)?;
        }
        drop(task_tx); // 关闭发送端

        // 收集结果
        let mut total_frames = 0;
        let mut processed_files = 0;
        let mut error_files = 0;

        while let Ok(result) = result_rx.recv().await {
            processed_files += 1;
            progress_bar.inc(1);

            match result.error {
                None => {
                    total_frames += result.frames.len();
                    crate::metrics::custom::record_file_parse_complete(0);
                    // 使用自定义的metrics函数替代直接调用
                    
                    // 保存解析结果
                    self.save_parse_result(&result).await?;
                }
                Some(error_msg) => {
                    error_files += 1;
                    error!("文件解析失败: {} - {}", result.file_path, error_msg);
                    crate::metrics::custom::record_parse_error("file_processing_error");
                }
            }

            // 更新进度条描述
            progress_bar.set_message(format!(
                "已处理: {}, 成功: {}, 错误: {}, 总帧数: {}",
                processed_files,
                processed_files - error_files,
                error_files,
                total_frames
            ));
        }

        // 等待所有工作线程完成
        while let Some(res) = join_set.join_next().await {
            res?;
        }

        progress_bar.finish_with_message(format!(
            "解析完成！处理文件: {}, 成功: {}, 错误: {}, 总帧数: {}",
            processed_files,
            processed_files - error_files,
            error_files,
            total_frames
        ));

        info!("解析完成！总帧数: {}", total_frames);
        Ok(())
    }

    /// 工作线程主循环
    async fn worker_loop(
        _worker_id: usize,
        mut task_rx: broadcast::Receiver<FileTask>,
        result_tx: broadcast::Sender<ParseResult>,
        semaphore: Arc<Semaphore>,
        frame_parser: Arc<FrameParser>,
        memory_pool: Arc<MemoryPool>,
        batch_size: usize,
    ) {
        let mut batch = VecDeque::with_capacity(batch_size);

        while let Ok(task) = task_rx.recv().await {
            // 获取信号量许可
            let _permit = semaphore.acquire().await.unwrap();

            // 处理单个文件
            let result = Self::process_single_file(&frame_parser, &memory_pool, task).await;
            
            batch.push_back(result);

            // 当批次满了或没有更多任务时，发送批次结果
            if batch.len() >= batch_size {
                Self::send_batch_results(&result_tx, &mut batch).await;
            }
        }

        // 发送剩余的结果
        Self::send_batch_results(&result_tx, &mut batch).await;
    }

    /// 处理单个文件
    async fn process_single_file(
        frame_parser: &FrameParser,
        memory_pool: &MemoryPool,
        task: FileTask,
    ) -> ParseResult {
        let start_time = std::time::Instant::now();

        match frame_parser.parse_file(&task.file_path).await {
            Ok(frames) => {
                let duration = start_time.elapsed();
                crate::metrics::custom::record_file_parse_complete(duration.as_millis() as u64);
                
                ParseResult {
                    file_path: task.file_path,
                    file_index: task.file_index,
                    frames,
                    error: None,
                }
            }
            Err(e) => {
                let duration = start_time.elapsed();
                crate::metrics::custom::record_parse_error("parse_timeout_error");
                
                ParseResult {
                    file_path: task.file_path,
                    file_index: task.file_index,
                    frames: Vec::new(),
                    error: Some(e.to_string()),
                }
            }
        }
    }

    /// 发送批次结果
    async fn send_batch_results(
        result_tx: &broadcast::Sender<ParseResult>,
        batch: &mut VecDeque<ParseResult>,
    ) {
        while let Some(result) = batch.pop_front() {
            if let Err(e) = result_tx.send(result) {
                error!("发送结果失败: {}", e);
                break;
            }
        }
    }

    /// 扫描.bin文件
    async fn scan_bin_files(&self) -> Result<Vec<String>> {
        let mut files = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.config.input_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "bin") {
                files.push(path.to_string_lossy().to_string());
            }
        }

        // 按文件名排序
        files.sort();
        Ok(files)
    }

    /// 保存解析结果
    async fn save_parse_result(&self, result: &ParseResult) -> Result<()> {
        if result.frames.is_empty() {
            return Ok(());
        }

        // 创建输出文件名
        let input_path = Path::new(&result.file_path);
        let file_stem = input_path.file_stem().unwrap().to_string_lossy();
        let output_file = self.config.output_dir.join(format!("{}.json", file_stem));

        // 序列化为JSON
        let json_data = serde_json::to_string_pretty(&result.frames)?;
        
        // 异步写入文件
        tokio::fs::write(output_file, json_data).await?;

        Ok(())
    }
} 