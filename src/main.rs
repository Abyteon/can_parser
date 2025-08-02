use anyhow::Result;
use clap::Parser;
use tracing::info;

mod config;
mod dbc_parser;
mod frame_parser;
mod memory_pool;
mod metrics;
mod worker;
mod file_cache;
mod benchmark;
mod performance_test;
mod high_performance_optimizer;
mod ultra_fast_worker;
mod async_pipeline;
mod concurrent_frame_parser;
mod batch_concurrent_parser;

use config::Config;
use worker::ParserWorker;
use std::sync::Arc;
use tracing::warn;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 输入目录路径
    #[arg(short, long)]
    input_dir: String,

    /// DBC文件路径
    #[arg(short, long)]
    dbc_file: String,

    /// 输出目录路径
    #[arg(short, long, default_value = "./output")]
    output_dir: String,

    /// 工作线程数量
    #[arg(short, long, default_value_t = num_cpus::get())]
    workers: usize,

    /// 批处理大小
    #[arg(short, long, default_value_t = 100)]
    batch_size: usize,

    /// 内存池大小 (MB)
    #[arg(short, long, default_value_t = 1024)]
    memory_pool_size: usize,

    /// 文件缓存最大条目数
    #[arg(long, default_value_t = 1000)]
    cache_entries: usize,

    /// 文件缓存TTL (秒)
    #[arg(long, default_value_t = 3600)]
    cache_ttl: u64,

    /// 运行性能评估
    #[arg(long)]
    performance_test: bool,

    /// 启用超高性能模式 (目标5分钟内完成)
    #[arg(long)]
    ultra_fast: bool,



    /// 启用优化的异步流水线模式 (推荐)
    #[arg(long)]
    async_pipeline: bool,

    /// 启用多层并发解析模式 (最新优化)
    #[arg(long)]
    concurrent_parse: bool,

    /// 启用批处理并发解析模式 (针对小数据块优化)
    #[arg(long)]
    batch_concurrent: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter("can_parser=info")
        .init();

    // 初始化指标收集
    metrics::init_metrics();

    let args = Args::parse();
    info!("启动CAN解析器，参数: {:?}", args);

    // 验证输入目录
    if !std::path::Path::new(&args.input_dir).exists() {
        anyhow::bail!("输入目录不存在: {}", args.input_dir);
    }

    // 验证DBC文件
    if !std::path::Path::new(&args.dbc_file).exists() {
        anyhow::bail!("DBC文件不存在: {}", args.dbc_file);
    }

    // 创建输出目录
    std::fs::create_dir_all(&args.output_dir)?;

    // 加载配置
    let mut config = Config::new(
        args.input_dir,
        args.dbc_file,
        args.output_dir,
        args.workers,
        args.batch_size,
        args.memory_pool_size,
    );
    
    // 更新缓存配置
    config.cache_max_entries = args.cache_entries;
    config.cache_ttl_seconds = args.cache_ttl;

    // 检查是否运行性能测试
    if args.performance_test {
        info!("运行性能评估模式...");
        let evaluator = performance_test::PerformanceEvaluator::new();
        let metrics = evaluator.run_comprehensive_evaluation().await?;
        evaluator.print_performance_report(&metrics);
        return Ok(());
    }

    // 检查是否启用批处理并发解析模式
    if args.batch_concurrent {
        info!("🚀 启动批处理并发解析模式 - 小数据块专用优化!");
        
        // 扫描输入文件
        let files = std::fs::read_dir(&config.input_dir)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "bin") {
                    Some(path.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if files.is_empty() {
            warn!("⚠️ 没有找到.bin文件");
            return Ok(());
        }

        // 初始化组件
        let memory_pool = Arc::new(memory_pool::MemoryPool::new(config.memory_pool_size_bytes() / (1024 * 1024)));
        let dbc_parser = Arc::new(dbc_parser::DbcParser::new());
        dbc_parser.load_dbc_file(&config.dbc_file.to_string_lossy()).await?;
        
        let file_cache = Arc::new(file_cache::FileCache::new(config.cache_max_entries, config.cache_ttl_seconds));
        let file_reader = file_cache::CachedFileReader::new(file_cache);

        // 创建批处理并发解析器 - 针对小数据块优化
        let batch_parser = batch_concurrent_parser::BatchConcurrentParserBuilder::new()
            .memory_pool(memory_pool.clone())
            .dbc_parser(dbc_parser.clone())
            .file_reader(file_reader)
            .decompress_batch_size(32)      // 32个压缩块一批
            .parse_batch_size(64)           // 64个数据块一批
            .frame_batch_size(128)          // 128个帧一批
            .worker_threads(args.workers)
            .small_block_threshold(8192)    // 8KB阈值
            .build()?;

        // 性能预估
        let avg_file_size_mb = 15.0; // 假设平均15MB
        let estimate = batch_parser.estimate_performance(files.len(), avg_file_size_mb);
        estimate.print_estimate();

        // 创建输出目录
        tokio::fs::create_dir_all(&config.output_dir).await?;

        // 批处理并发解析所有文件
        info!("🔄 开始批处理并发解析 {} 个文件...", files.len());
        let start_time = std::time::Instant::now();
        
        for (i, file_path) in files.iter().enumerate() {
            let file_name = std::path::Path::new(file_path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            
            info!("📝 处理文件 {}/{}: {}", i + 1, files.len(), file_name);
            
            match batch_parser.parse_file_batch_concurrent(file_path).await {
                Ok(frames) => {
                    let output_path = config.output_dir.join(format!("{}.json", file_name));
                    let json_data = serde_json::to_vec_pretty(&frames)?;
                    tokio::fs::write(&output_path, json_data).await?;
                    info!("✅ 文件 {} 处理完成: {} 帧", file_name, frames.len());
                }
                Err(e) => {
                    warn!("❌ 文件 {} 处理失败: {}", file_name, e);
                }
            }
        }

        let total_time = start_time.elapsed();
        let throughput = files.len() as f64 / total_time.as_secs_f64();
        info!("🎉 批处理并发解析完成!");
        info!("📊 总计: {} 文件, 耗时 {:.2}s, 吞吐量 {:.1} 文件/秒", 
              files.len(), total_time.as_secs_f64(), throughput);

        if total_time.as_secs_f64() <= 300.0 {
            info!("✅ 在5分钟内完成！实际用时 {:.1} 分钟", total_time.as_secs_f64() / 60.0);
        } else {
            warn!("⏰ 超过5分钟目标，用时 {:.1} 分钟", total_time.as_secs_f64() / 60.0);
        }

        return Ok(());
    }

    // 检查是否启用多层并发解析模式
    if args.concurrent_parse {
        info!("🚀 启动多层并发解析模式 - 最新优化!");
        
        // 扫描输入文件
        let files = std::fs::read_dir(&config.input_dir)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "bin") {
                    Some(path.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if files.is_empty() {
            warn!("⚠️ 没有找到.bin文件");
            return Ok(());
        }

        // 初始化组件
        let memory_pool = Arc::new(memory_pool::MemoryPool::new(config.memory_pool_size_bytes() / (1024 * 1024)));
        let dbc_parser = Arc::new(dbc_parser::DbcParser::new());
        dbc_parser.load_dbc_file(&config.dbc_file.to_string_lossy()).await?;
        
        let file_cache = Arc::new(file_cache::FileCache::new(config.cache_max_entries, config.cache_ttl_seconds));
        let file_reader = file_cache::CachedFileReader::new(file_cache);

        // 创建并发解析器
        let concurrent_parser = concurrent_frame_parser::ConcurrentFrameParserBuilder::new()
            .memory_pool(memory_pool.clone())
            .dbc_parser(dbc_parser.clone())
            .file_reader(file_reader)
            .decompress_workers(args.workers)
            .parse_workers(args.workers * 2)
            .frame_workers(args.workers * 2)
            .batch_size(64)
            .build()?;

        // 使用异步流水线 + 并发解析器
        let frame_parser = Arc::new(frame_parser::FrameParser::with_file_reader(memory_pool.clone(), dbc_parser, file_cache::CachedFileReader::new(Arc::new(file_cache::FileCache::new(100, 3600)))));
        
        let pipeline = async_pipeline::AsyncPipeline::with_concurrent_parser(
            memory_pool,
            frame_parser,
            Arc::new(concurrent_parser),
            config.output_dir.clone(),
            Some(async_pipeline::PipelineConfig {
                io_workers: args.workers * 2,
                cpu_workers: args.workers,
                write_workers: args.workers / 2,
                channel_buffer: 1000,
            }),
        );

        // 运行流水线
        pipeline.process_files(files).await?;
        return Ok(());
    }

    // 检查是否启用优化的异步流水线模式
    if args.async_pipeline {
        info!("🚀 启动优化的异步流水线模式 - 推荐架构!");
        
        // 扫描输入文件
        let files = std::fs::read_dir(&config.input_dir)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "bin") {
                    Some(path.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if files.is_empty() {
            warn!("⚠️ 没有找到.bin文件");
            return Ok(());
        }

        // 初始化组件 - 复用现有架构
        let memory_pool = Arc::new(memory_pool::MemoryPool::new(config.memory_pool_size_bytes() / (1024 * 1024)));
        let dbc_parser = Arc::new(dbc_parser::DbcParser::new());
        dbc_parser.load_dbc_file(&config.dbc_file.to_string_lossy()).await?;
        
        let file_cache = Arc::new(file_cache::FileCache::new(config.cache_max_entries, config.cache_ttl_seconds));
        let file_reader = file_cache::CachedFileReader::new(file_cache);
        let frame_parser = Arc::new(frame_parser::FrameParser::with_file_reader(memory_pool.clone(), dbc_parser, file_reader));

        // 使用构建器模式创建异步流水线
        let pipeline = async_pipeline::AsyncPipelineBuilder::new()
            .memory_pool(memory_pool)
            .frame_parser(frame_parser)
            .output_dir(config.output_dir.clone())
            .io_workers(args.workers * 2)
            .cpu_workers(args.workers)
            .write_workers(args.workers / 2)
            .channel_buffer(1000)
            .build()?;

        // 运行流水线
        pipeline.process_files(files).await?;
        return Ok(());
    }



    // 检查是否启用超高性能模式
    if args.ultra_fast {
        info!("🚀 启动超高性能模式 - 目标5分钟内完成!");
        let ultra_worker = ultra_fast_worker::UltraFastWorker::new(config).await?;
        ultra_worker.run_ultra_fast().await?;
        return Ok(());
    }

    // 启动解析工作器
    let worker = ParserWorker::new(config).await?;
    
    info!("开始解析文件...");
    worker.run().await?;
    
    info!("解析完成！");
    Ok(())
} 