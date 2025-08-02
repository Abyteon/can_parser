use std::sync::Arc;
use tokio::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 高性能内存池
pub struct MemoryPool {
    /// 缓冲区对象池
    buffer_pool: Arc<Mutex<Vec<Vec<u8>>>>,
    /// 字符串对象池
    string_pool: Arc<Mutex<Vec<String>>>,
    /// 统计信息
    stats: Arc<InternalPoolStats>,
}

#[derive(Debug)]
struct InternalPoolStats {
    total_allocated: AtomicUsize,
    total_freed: AtomicUsize,
    current_usage: AtomicUsize,
    peak_usage: AtomicUsize,
}

impl InternalPoolStats {
    fn new() -> Self {
        Self {
            total_allocated: AtomicUsize::new(0),
            total_freed: AtomicUsize::new(0),
            current_usage: AtomicUsize::new(0),
            peak_usage: AtomicUsize::new(0),
        }
    }

    fn record_allocation(&self, size: usize) {
        self.total_allocated.fetch_add(size, Ordering::Relaxed);
        let current = self.current_usage.fetch_add(size, Ordering::Relaxed) + size;
        let mut peak = self.peak_usage.load(Ordering::Relaxed);
        while current > peak {
            match self.peak_usage.compare_exchange_weak(
                peak, current, Ordering::Relaxed, Ordering::Relaxed
            ) {
                Ok(_) => break,
                Err(new_peak) => peak = new_peak,
            }
        }
    }

    fn record_deallocation(&self, size: usize) {
        self.total_freed.fetch_add(size, Ordering::Relaxed);
        self.current_usage.fetch_sub(size, Ordering::Relaxed);
    }
}

impl MemoryPool {
    pub fn new(pool_size: usize) -> Self {
        let mut buffer_pool = Vec::with_capacity(pool_size * 2);
        let mut string_pool = Vec::with_capacity(pool_size);
        
        // 预填充池
        for _ in 0..pool_size * 2 {
            buffer_pool.push(Vec::with_capacity(1024));
        }
        for _ in 0..pool_size {
            string_pool.push(String::with_capacity(256));
        }

        Self {
            buffer_pool: Arc::new(Mutex::new(buffer_pool)),
            string_pool: Arc::new(Mutex::new(string_pool)),
            stats: Arc::new(InternalPoolStats::new()),
        }
    }

    /// 从池中获取缓冲区
    pub async fn get_buffer(&self) -> Option<Vec<u8>> {
        let mut pool = self.buffer_pool.lock().await;
        let result = pool.pop();
        if result.is_some() {
            self.stats.record_allocation(1024);
        }
        result
    }

    /// 将缓冲区归还到池中
    pub async fn return_buffer(&self, mut buffer: Vec<u8>) {
        buffer.clear();
        let mut pool = self.buffer_pool.lock().await;
        if pool.len() < pool.capacity() {
            pool.push(buffer);
            self.stats.record_deallocation(1024);
        }
    }

    /// 从池中获取字符串
    pub async fn get_string(&self) -> Option<String> {
        let mut pool = self.string_pool.lock().await;
        let result = pool.pop();
        if result.is_some() {
            self.stats.record_allocation(256);
        }
        result
    }

    /// 将字符串归还到池中
    pub async fn return_string(&self, mut string: String) {
        string.clear();
        let mut pool = self.string_pool.lock().await;
        if pool.len() < pool.capacity() {
            pool.push(string);
            self.stats.record_deallocation(256);
        }
    }

    /// 获取池统计信息
    pub async fn stats(&self) -> PoolStats {
        PoolStats {
            available_buffers: self.buffer_pool.lock().await.len(),
            total_pool_size: self.buffer_pool.lock().await.capacity(),
            total_allocated: self.stats.total_allocated.load(Ordering::Relaxed),
            total_freed: self.stats.total_freed.load(Ordering::Relaxed),
            current_usage: self.stats.current_usage.load(Ordering::Relaxed),
            peak_usage: self.stats.peak_usage.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PoolStats {
    pub available_buffers: usize,
    pub total_pool_size: usize,
    pub total_allocated: usize,
    pub total_freed: usize,
    pub current_usage: usize,
    pub peak_usage: usize,
}

impl Default for MemoryPool {
    fn default() -> Self {
        Self::new(1000)
    }
} 