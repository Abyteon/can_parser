bai.tn@Baitn ~/D/can_parser (main)> time ./target/relea
se/can_parser --input-dir large_test_data --dbc-file si
mple.dbc --output-dir large_output_data --ultra-fast --
workers 16 --memory-pool-size 8192 --cache-entries 1000
2025-08-02T17:30:19.961204Z INFO can_parser: 启动CAN解析器，参数: Args { input_dir: "large_test_data", dbc_file: "simple.dbc", output_dir: "large_output_data", workers: 16, batch_size: 100, memory_pool_size: 8192, cache_entries: 1000, cache_ttl: 3600, performance_test: false, ultra_fast: true }
2025-08-02T17:30:19.961241Z INFO can_parser: 🚀 启动超高性能模式 - 目标5分钟内完成!
2025-08-02T17:30:19.961243Z INFO can_parser::ultra_fast_worker: 🚀 初始化超高性能工作器...
2025-08-02T17:30:19.961939Z INFO can_parser::ultra_fast_worker: ✅ 超高性能工作器初始化完成
2025-08-02T17:30:19.961945Z INFO can_parser::ultra_fast_worker: 📊 配置: CPU核心=10, 内存池=16384MB, 缓存=2000个文件
2025-08-02T17:30:19.961947Z INFO can_parser::ultra_fast_worker: 🔍 开始超高速扫描输入目录...
2025-08-02T17:30:19.962479Z INFO can_parser::ultra_fast_worker: 📁 发现 800 个.bin文件
2025-08-02T17:30:19.962484Z INFO can_parser::ultra_fast_worker: 🔥 预热缓存和内存池...
2025-08-02T17:30:19.962486Z INFO can_parser::ultra_fast_worker: 🔥 预热 50 个文件到缓存...
2025-08-02T17:30:19.962557Z INFO can_parser::ultra_fast_worker: ✅ 系统预热完成
2025-08-02T17:30:19.962586Z INFO can_parser::ultra_fast_worker: 🚀 启动超高性能批处理模式...
2025-08-02T17:30:19.962590Z INFO can_parser::ultra_fast_worker: 📦 使用批次大小: 32
⠁ [00:00:03] [█▌ ] 32/800 (77s) 批次 1/25 | 10 文件/秒 | 总计: 32 2025-08-02T17:30:23.184332Z INFO can_parser::ultra_fas
⠉ [00:00:07] [███▏ ] 64/800 (85s) 批次 2/25 | 10 文件/秒 | 总计: 64 2025-08-02T17:30:27.554704Z INFO can_parser::ultra_fas
⠙ [00:00:11] [████▊ ] 96/800 (85s) 批次 3/25 | 11 文件/秒 | 总计: 96 2025-08-02T17:30:31.624614Z INFO can_parser::ultra_fas
⠚ [00:00:15] [██████▍ ] 128/800 (83s) 批次 4/25 | 11 文件/秒 | 总计: 128 2025-08-02T17:30:35.579183Z INFO can_parser::ultra_fas
⠒ [00:00:19] [████████ ] 160/800 (80s) 批次 5/25 | 11 文件/秒 | 总计: 160 2025-08-02T17:30:39.769414Z INFO can_parser::ultra_fas
⠂ [00:00:23] [█████████▌ ] 192/800 (77s) 批次 6/25 | 11 文件/秒 | 总计: 192 2025-08-02T17:30:43.885140Z INFO can_parser::ultra_fas
⠂ [00:00:27] [███████████▏ ] 224/800 (73s) 批次 7/25 | 11 文件/秒 | 总计: 224 2025-08-02T17:30:47.936786Z INFO can_parser::ultra_fas
⠒ [00:00:32] [████████████▊ ] 256/800 (69s) 批次 8/25 | 11 文件/秒 | 总计: 256 2025-08-02T17:30:52.066967Z INFO can_parser::ultra_fas
⠲ [00:00:36] [██████████████▍ ] 288/800 (66s) 批次 9/25 | 10 文件/秒 | 总计: 288 2025-08-02T17:30:56.440863Z INFO can_parser::ultra_fas
⠴ [00:00:40] [████████████████ ] 320/800 (63s) 批次 10/25 | 11 文件/秒 | 总计: 320 2025-08-02T17:31:00.695220Z INFO can_parser::ultra_fas
⠤ [00:00:44] [█████████████████▌ ] 352/800 (58s) 批次 11/25 | 11 文件/秒 | 总计: 352 2025-08-02T17:31:04.754463Z INFO can_parser::ultra_fas
⠄ [00:00:48] [███████████████████▏ ] 384/800 (54s) 批次 12/25 | 11 文件/秒 | 总计: 384 2025-08-02T17:31:08.818932Z INFO can_parser::ultra_fas
⠄ [00:00:52] [████████████████████▊ ] 416/800 (49s) 批次 13/25 | 11 文件/秒 | 总计: 416 2025-08-02T17:31:12.825349Z INFO can_parser::ultra_fas
⠤ [00:00:57] [██████████████████████▍ ] 448/800 (45s) 批次 14/25 | 11 文件/秒 | 总计: 448 2025-08-02T17:31:17.056276Z INFO can_parser::ultra_fas
⠠ [00:01:01] [████████████████████████ ] 480/800 (41s) 批次 15/25 | 11 文件/秒 | 总计: 480 2025-08-02T17:31:21.148018Z INFO can_parser::ultra_fas
⠠ [00:01:05] [█████████████████████████▌ ] 512/800 (37s) 批次 16/25 | 10 文件/秒 | 总计: 512 2025-08-02T17:31:25.433925Z INFO can_parser::ultra_fas
⠤ [00:01:09] [███████████████████████████▏ ] 544/800 (34s) 批次 17/25 | 10 文件/秒 | 总计: 544 2025-08-02T17:31:29.801242Z INFO can_parser::ultra_fas
⠦ [00:01:14] [████████████████████████████▊ ] 576/800 (30s) 批次 18/25 | 10 文件/秒 | 总计: 576 2025-08-02T17:31:34.384113Z INFO can_parser::ultra_fas
⠖ [00:01:18] [██████████████████████████████▍ ] 608/800 (26s) 批次 19/25 | 11 文件/秒 | 总计: 608 2025-08-02T17:31:38.465278Z INFO can_parser::ultra_fas
⠒ [00:01:23] [████████████████████████████████ ] 640/800 (22s) 批次 20/25 | 10 文件/秒 | 总计: 640 2025-08-02T17:31:43.137641Z INFO can_parser::ultra_fas
⠐ [00:01:28] [█████████████████████████████████▌ ] 672/800 (18s) 批次 21/25 | 9 文件/秒 | 总计: 672 2025-08-02T17:31:48.287522Z INFO can_parser::ultra_fas
⠐ [00:01:32] [███████████████████████████████████▏ ] 704/800 (14s) 批次 22/25 | 10 文件/秒 | 总计: 704 2025-08-02T17:31:52.898095Z INFO can_parser::ultra_fas
⠒ [00:01:38] [████████████████████████████████████▊ ] 736/800 (10s) 批次 23/25 | 9 文件/秒 | 总计: 736 2025-08-02T17:31:57.979758Z INFO can_parser::ultra_fas
⠓ [00:01:43] [██████████████████████████████████████▍ ] 768/800 (5s) 批次 24/25 | 9 文件/秒 | 总计: 768 2025-08-02T17:32:03.103025Z INFO can_parser::ultra_fas
⠋ [00:01:47] [████████████████████████████████████████] 800/800 (0s) 批次 25/25 | 10 文件/秒 | 总计: 800 📈 进度检查: 800/800 (100.0%), 预计总耗时: 108.0s
2025-08-02T17:32:07.912123Z INFO can_parser::ultra_fas
[00:01:49] [████████████████████████████████████████] 800/800 (0s) 🎉 处理完成! 800 文件 | 109.50s | 7 文件/秒 2025-08-02T17:32:09.464095Z INFO can_parser::ultra_fast_worker: 📊 === 超高性能处理完成 ===
2025-08-02T17:32:09.464101Z INFO can_parser::ultra_fast_worker: ⏱️ 总耗时: 109.50 秒
2025-08-02T17:32:09.464103Z INFO can_parser::ultra_fast_worker: 🔥 平均吞吐量: 7 文件/秒
2025-08-02T17:32:09.464110Z INFO can_parser::ultra_fast_worker: 💾 峰值内存使用: ~46.546875GB
2025-08-02T17:32:09.464112Z INFO can_parser::ultra_fast_worker: 🎯 目标达成! 在 109.50s 内完成处理 (目标: 300s)

---

Executed in 109.83 secs fish external
usr time 240.05 secs 144.00 micros 240.05 secs
sys time 117.98 secs 735.00 micros 117.98 secs
