#!/usr/bin/env python3
"""
为CAN解析器创建测试文件
macOS兼容版本
"""

import os
import random

def create_test_files():
    # 创建测试目录
    test_dir = "test_data"
    os.makedirs(test_dir, exist_ok=True)
    
    print("🚀 开始创建测试文件...")
    
    # 创建不同大小的测试文件
    file_configs = [
        {"count": 20, "size_mb": 1, "prefix": "small"},      # 20个1MB文件
        {"count": 10, "size_mb": 5, "prefix": "medium"},     # 10个5MB文件  
        {"count": 5, "size_mb": 15, "prefix": "large"},      # 5个15MB文件
    ]
    
    total_files = 0
    total_size_mb = 0
    
    for config in file_configs:
        print(f"📁 创建 {config['count']} 个 {config['size_mb']}MB 的 {config['prefix']} 文件...")
        
        for i in range(1, config['count'] + 1):
            filename = f"{test_dir}/{config['prefix']}_test_{i:03d}.bin"
            size_bytes = config['size_mb'] * 1024 * 1024
            
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
            
            total_files += 1
            total_size_mb += config['size_mb']
            
            if i % 5 == 0:  # 每5个文件报告一次进度
                print(f"   ✅ 已创建 {i}/{config['count']} 个文件")
    
    print(f"\n🎉 测试文件创建完成!")
    print(f"📊 总计: {total_files} 个文件, {total_size_mb}MB")
    print(f"📂 位置: {os.path.abspath(test_dir)}")
    
    # 列出创建的文件
    files = os.listdir(test_dir)
    bin_files = [f for f in files if f.endswith('.bin')]
    print(f"\n📋 .bin文件列表 (前10个):")
    for f in sorted(bin_files)[:10]:
        size_mb = os.path.getsize(os.path.join(test_dir, f)) / (1024 * 1024)
        print(f"   {f} ({size_mb:.1f}MB)")
    
    if len(bin_files) > 10:
        print(f"   ... 还有 {len(bin_files) - 10} 个文件")

if __name__ == "__main__":
    create_test_files()