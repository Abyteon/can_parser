use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use memmap2::Mmap;
use std::fs::File;
use cached::{Cached, TimedCache};
use std::sync::Mutex;

/// 文件缓存统计
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub total_size: usize,
    pub cache_size: usize,
}

/// 高性能文件缓存
pub struct FileCache {
    /// 缓存实例
    cache: Arc<Mutex<TimedCache<PathBuf, Mmap>>>,
    /// 统计信息
    stats: Arc<Mutex<CacheStats>>,
}

impl FileCache {
    pub fn new(max_entries: usize, ttl_seconds: u64) -> Self {
        let cache = TimedCache::with_lifespan_and_capacity(
            std::time::Duration::from_secs(ttl_seconds), 
            max_entries
        );
        
        Self {
            cache: Arc::new(Mutex::new(cache)),
            stats: Arc::new(Mutex::new(CacheStats {
                hits: 0,
                misses: 0,
                evictions: 0,
                total_size: 0,
                cache_size: 0,
            })),
        }
    }

    /// 获取文件内容
    pub async fn get(&self, path: &Path) -> Result<Option<&'static [u8]>> {
        let path_buf = path.to_path_buf();
        
        // 检查缓存
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(mmap) = cache.cache_get(&path_buf) {
                let mut stats = self.stats.lock().unwrap();
                stats.hits += 1;
                
                // 返回静态引用
                let ptr = mmap.as_ptr();
                let len = mmap.len();
                return Ok(Some(unsafe { std::slice::from_raw_parts(ptr, len) }));
            }
        }

        // 缓存未命中，加载文件
        let mut stats = self.stats.lock().unwrap();
        stats.misses += 1;

        // 检查文件是否存在
        if !path.exists() {
            return Ok(None);
        }

        // 加载文件到内存映射
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let size = mmap.len();

        // 添加到缓存
        {
            let mut cache = self.cache.lock().unwrap();
            cache.cache_set(path_buf.clone(), mmap);
            stats.total_size += size;
            stats.cache_size = cache.cache_size();
        }

        // 返回新加载的内容
        let mut cache = self.cache.lock().unwrap();
        if let Some(mmap) = cache.cache_get(&path_buf) {
            let ptr = mmap.as_ptr();
            let len = mmap.len();
            Ok(Some(unsafe { std::slice::from_raw_parts(ptr, len) }))
        } else {
            Ok(None)
        }
    }

    /// 预加载文件到缓存
    pub async fn preload(&self, paths: Vec<PathBuf>) -> Result<()> {
        for path in paths {
            if path.exists() {
                let _ = self.get(&path).await?;
            }
        }
        Ok(())
    }

    /// 获取缓存统计信息
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.lock().unwrap();
        let stats = self.stats.lock().unwrap();
        CacheStats {
            hits: cache.cache_hits().unwrap_or(0),
            misses: cache.cache_misses().unwrap_or(0),
            evictions: 0, // cached库没有evictions方法
            total_size: stats.total_size,
            cache_size: cache.cache_size(),
        }
    }

    /// 清空缓存
    pub async fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.cache_clear();
        let mut stats = self.stats.lock().unwrap();
        stats.total_size = 0;
    }

    /// 预热缓存
    pub async fn warmup(&self, directory: &Path) -> Result<()> {
        let mut paths = Vec::new();
        
        if directory.is_dir() {
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "bin") {
                    paths.push(path);
                }
            }
        }

        // 按文件大小排序，优先加载小文件
        paths.sort_by(|a, b| {
            let size_a = std::fs::metadata(a).map(|m| m.len()).unwrap_or(0);
            let size_b = std::fs::metadata(b).map(|m| m.len()).unwrap_or(0);
            size_a.cmp(&size_b)
        });

        // 预加载前N个文件
        let preload_count = std::cmp::min(paths.len(), 100);
        for path in paths.into_iter().take(preload_count) {
            let _ = self.get(&path).await?;
        }

        Ok(())
    }
}

impl Default for FileCache {
    fn default() -> Self {
        Self::new(1000, 3600) // 1000个文件, 1小时TTL
    }
}

/// 文件读取器，支持缓存
#[derive(Clone)]
pub struct CachedFileReader {
    cache: Arc<FileCache>,
}

impl CachedFileReader {
    pub fn new(cache: Arc<FileCache>) -> Self {
        Self { cache }
    }

    /// 读取文件内容
    pub async fn read_file(&self, path: &Path) -> Result<Option<&'static [u8]>> {
        self.cache.get(path).await
    }

    /// 批量读取文件
    pub async fn read_files(&self, paths: &[PathBuf]) -> Result<Vec<Option<&'static [u8]>>> {
        let mut results = Vec::with_capacity(paths.len());
        
        for path in paths {
            let content = self.cache.get(path).await?;
            results.push(content);
        }
        
        Ok(results)
    }
} 