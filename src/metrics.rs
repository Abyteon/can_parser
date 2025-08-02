use metrics_exporter_prometheus::PrometheusBuilder;
use std::net::SocketAddr;
use tokio::task::JoinHandle;

/// 初始化指标收集
pub fn init_metrics() {
    // 注册Prometheus指标
    let builder = PrometheusBuilder::new();
    builder.install().unwrap();
}

/// 启动指标服务器
pub async fn start_metrics_server(addr: SocketAddr) -> Result<JoinHandle<()>, std::io::Error> {
    let handle = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        println!("指标服务器运行在 http://{}", addr);

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let _peer_addr = stream.peer_addr().unwrap();
            
            tokio::spawn(async move {
                let mut stream = tokio::io::BufStream::new(stream);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n{}",
                    "Metrics endpoint placeholder"
                );
                
                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
            });
        }
    });

    Ok(handle)
}

/// 自定义指标
pub mod custom {
    /// 记录文件解析开始
    pub fn record_file_parse_start() {
        metrics::counter!("file_parse_started").increment(1);
    }

    /// 记录文件解析完成
    pub fn record_file_parse_complete(duration_ms: u64) {
        metrics::counter!("file_parse_completed").increment(1);
        metrics::histogram!("file_parse_duration_ms").record(duration_ms as f64);
    }

    /// 记录内存池使用情况
    pub fn record_memory_pool_usage(used: usize, total: usize) {
        metrics::gauge!("memory_pool_used_bytes").set(used as f64);
        metrics::gauge!("memory_pool_total_bytes").set(total as f64);
        metrics::gauge!("memory_pool_usage_ratio").set(used as f64 / total as f64);
    }

    /// 记录工作线程状态
    pub fn record_worker_status(active_workers: usize, total_workers: usize) {
        metrics::gauge!("active_workers").set(active_workers as f64);
        metrics::gauge!("total_workers").set(total_workers as f64);
    }

    /// 记录解析错误
    pub fn record_parse_error(error_type: &'static str) {
        let labels = [("type", error_type.to_string())];
        metrics::counter!("parse_errors", &labels).increment(1);
    }

    /// 记录CAN帧解析统计
    pub fn record_can_frame_stats(total_frames: u64, valid_frames: u64, invalid_frames: u64) {
        metrics::counter!("total_can_frames").increment(total_frames);
        metrics::counter!("valid_can_frames").increment(valid_frames);
        metrics::counter!("invalid_can_frames").increment(invalid_frames);
    }
} 