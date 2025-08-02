#!/usr/bin/env python3
"""
创建大规模测试文件集来验证超高性能模式
模拟8000个15MB文件的场景
"""

import os
import random
import time
from concurrent.futures import ProcessPoolExecutor
import multiprocessing

def create_file_batch(args):
    """在单独进程中创建一批文件"""
    start_idx, end_idx, size_mb, test_dir = args
    
    for i in range(start_idx, end_idx):
        filename = f"{test_dir}/large_scale_test_{i:04d}.bin"
        size_bytes = size_mb * 1024 * 1024
        
        # 创建随机二进制数据
        with open(filename, 'wb') as f:
            # 分块写入以避免内存问题
            chunk_size = 64 * 1024  # 64KB块
            chunks = size_bytes // chunk_size
            remaining = size_bytes % chunk_size
            
            for _ in range(chunks):
                data = bytes([random.randint(0, 255) for _ in range(chunk_size)])
                f.write(data)
            
            if remaining > 0:
                data = bytes([random.randint(0, 255) for _ in range(remaining)])
                f.write(data)
    
    return f"批次 {start_idx}-{end_idx-1} 完成"

def create_large_scale_test():
    """创建大规模测试文件"""
    test_dir = "large_test_data"
    os.makedirs(test_dir, exist_ok=True)
    
    # 配置
    total_files = 800  # 先创建800个文件（8000个太多，会占用太多磁盘空间）
    file_size_mb = 15
    batch_size = 50  # 每批50个文件
    num_processes = min(multiprocessing.cpu_count(), 8)  # 使用多进程并行创建
    
    print(f"🚀 开始创建大规模测试数据集...")
    print(f"📊 总文件数: {total_files}")
    print(f"📄 单文件大小: {file_size_mb}MB")
    print(f"💾 总数据量: {total_files * file_size_mb}MB ({total_files * file_size_mb / 1024:.1f}GB)")
    print(f"🔧 并行进程数: {num_processes}")
    
    start_time = time.time()
    
    # 准备批次参数
    batches = []
    for i in range(0, total_files, batch_size):
        end_idx = min(i + batch_size, total_files)
        batches.append((i, end_idx, file_size_mb, test_dir))
    
    print(f"📦 将创建 {len(batches)} 个批次...")
    
    # 使用多进程并行创建文件
    completed_batches = 0
    with ProcessPoolExecutor(max_workers=num_processes) as executor:
        for result in executor.map(create_file_batch, batches):
            completed_batches += 1
            print(f"✅ {result} ({completed_batches}/{len(batches)})")
    
    end_time = time.time()
    creation_time = end_time - start_time
    
    print(f"\n🎉 大规模测试数据集创建完成!")
    print(f"⏱️  创建耗时: {creation_time:.2f}秒")
    print(f"🚀 创建速度: {total_files / creation_time:.1f} 文件/秒")
    print(f"📂 位置: {os.path.abspath(test_dir)}")
    
    # 验证文件
    files = [f for f in os.listdir(test_dir) if f.endswith('.bin')]
    total_size_gb = sum(os.path.getsize(os.path.join(test_dir, f)) for f in files) / (1024**3)
    
    print(f"\n📋 验证结果:")
    print(f"   ✅ 文件数量: {len(files)}")
    print(f"   ✅ 总大小: {total_size_gb:.2f}GB")
    print(f"   ✅ 平均文件大小: {total_size_gb * 1024 / len(files):.1f}MB")

if __name__ == "__main__":
    create_large_scale_test()