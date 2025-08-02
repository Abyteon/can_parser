use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// DBC信号定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub name: String,
    pub start_bit: u32,
    pub length: u32,
    pub byte_order: ByteOrder,
    pub value_type: ValueType,
    pub factor: f64,
    pub offset: f64,
    pub min_value: f64,
    pub max_value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ByteOrder {
    LittleEndian,
    BigEndian,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValueType {
    Signed,
    Unsigned,
    Float,
}

/// DBC消息定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: u32,
    pub name: String,
    pub length: u32,
    pub signals: HashMap<String, Signal>,
}

/// DBC解析器
pub struct DbcParser {
    messages: Arc<RwLock<HashMap<u32, Message>>>,
}

impl DbcParser {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 加载DBC文件
    pub async fn load_dbc_file(&self, file_path: &str) -> Result<()> {
        let content = tokio::fs::read_to_string(file_path).await?;
        self.parse_dbc_content(&content).await
    }

    /// 解析DBC文件内容
    async fn parse_dbc_content(&self, content: &str) -> Result<()> {
        let mut messages = self.messages.write().await;
        
        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();
            
            if line.starts_with("BO_") {
                // 解析消息定义
                let message = self.parse_message_definition(line)?;
                let message_id = message.id;
                
                // 解析该消息的所有信号
                let mut signals = HashMap::new();
                i += 1;
                
                while i < lines.len() {
                    let signal_line = lines[i].trim();
                    if signal_line.starts_with("SG_") {
                        let signal = self.parse_signal_definition(signal_line)?;
                        signals.insert(signal.name.clone(), signal);
                        i += 1;
                    } else if signal_line.starts_with("BO_") || signal_line.is_empty() {
                        break;
                    } else {
                        i += 1;
                    }
                }
                
                let mut message = message;
                message.signals = signals;
                messages.insert(message_id, message);
            } else {
                i += 1;
            }
        }

        Ok(())
    }

    /// 解析消息定义行
    fn parse_message_definition(&self, line: &str) -> Result<Message> {
        // 格式: BO_ 100 EngineData: 8 Vector__XXX
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            return Err(anyhow!("无效的消息定义行: {}", line));
        }

        let id = parts[1].parse::<u32>()?;
        let name = parts[2].trim_end_matches(':').to_string();
        let length = parts[3].parse::<u32>()?;

        Ok(Message {
            id,
            name,
            length,
            signals: HashMap::new(),
        })
    }

    /// 解析信号定义行
    fn parse_signal_definition(&self, line: &str) -> Result<Signal> {
        // 格式: SG_ EngineSpeed : 0|16@1+ (0.125,0) [0|8031.875] "rpm" Vector__XXX
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            return Err(anyhow!("无效的信号定义行: {}", line));
        }

        let name = parts[1].to_string();
        
        // 解析位定义: 0|16@1+
        let bit_def = parts[3];
        let bit_parts: Vec<&str> = bit_def.split('|').collect();
        if bit_parts.len() != 2 {
            return Err(anyhow!("无效的位定义: {}", bit_def));
        }

        let start_bit = bit_parts[0].parse::<u32>()?;
        
        let length_order = bit_parts[1];
        let at_pos = length_order.find('@').ok_or_else(|| anyhow!("无效的长度和字节序定义"))?;
        let length = length_order[..at_pos].parse::<u32>()?;
        
        let byte_order = if length_order.contains('@') {
            let order_part = &length_order[at_pos + 1..];
            if order_part.starts_with('0') {
                ByteOrder::LittleEndian
            } else {
                ByteOrder::BigEndian
            }
        } else {
            ByteOrder::LittleEndian
        };

        let value_type = if length_order.ends_with('+') {
            ValueType::Unsigned
        } else {
            ValueType::Signed
        };

        // 解析因子和偏移: (0.125,0)
        let mut factor = 1.0;
        let mut offset = 0.0;
        if parts.len() > 4 && parts[4].starts_with('(') {
            let factor_offset = parts[4].trim_matches('(').trim_matches(')');
            let factor_parts: Vec<&str> = factor_offset.split(',').collect();
            if factor_parts.len() == 2 {
                factor = factor_parts[0].parse::<f64>().unwrap_or(1.0);
                offset = factor_parts[1].parse::<f64>().unwrap_or(0.0);
            }
        }

        // 解析范围: [0|8031.875]
        let mut min_value = 0.0;
        let mut max_value = f64::MAX;
        if parts.len() > 5 && parts[5].starts_with('[') {
            let range = parts[5].trim_matches('[').trim_matches(']');
            let range_parts: Vec<&str> = range.split('|').collect();
            if range_parts.len() == 2 {
                min_value = range_parts[0].parse::<f64>().unwrap_or(0.0);
                max_value = range_parts[1].parse::<f64>().unwrap_or(f64::MAX);
            }
        }

        // 解析单位
        let unit = if parts.len() > 6 {
            parts[6].trim_matches('"').to_string()
        } else {
            "".to_string()
        };

        Ok(Signal {
            name,
            start_bit,
            length,
            byte_order,
            value_type,
            factor,
            offset,
            min_value,
            max_value,
            unit,
        })
    }

    /// 解析CAN信号
    pub async fn parse_signals(&self, can_id: u32, data: &[u8]) -> Result<HashMap<String, f64>> {
        let messages = self.messages.read().await;
        
        if let Some(message) = messages.get(&can_id) {
            let mut signals = HashMap::new();
            
            for (signal_name, signal) in &message.signals {
                if let Ok(value) = self.extract_signal_value(data, signal) {
                    signals.insert(signal_name.clone(), value);
                }
            }
            
            Ok(signals)
        } else {
            Ok(HashMap::new())
        }
    }

    /// 从CAN数据中提取信号值
    fn extract_signal_value(&self, data: &[u8], signal: &Signal) -> Result<f64> {
        let start_byte = (signal.start_bit / 8) as usize;
        let start_bit_in_byte = signal.start_bit % 8;
        
        if start_byte + ((signal.length + 7) / 8) as usize > data.len() {
            return Err(anyhow!("信号超出数据范围"));
        }

        let mut raw_value: u64 = 0;
        let bytes_needed = (signal.length + 7) / 8;

        match signal.byte_order {
            ByteOrder::LittleEndian => {
                for i in 0..bytes_needed {
                    raw_value |= (data[start_byte + i as usize] as u64) << (i * 8);
                }
            }
            ByteOrder::BigEndian => {
                for i in 0..bytes_needed {
                    raw_value |= (data[start_byte + i as usize] as u64) << ((bytes_needed - 1 - i) * 8);
                }
            }
        }

        // 应用位掩码
        let mask = (1u64 << signal.length) - 1;
        raw_value = (raw_value >> start_bit_in_byte) & mask;

        // 转换为有符号或无符号值
        let value = match signal.value_type {
            ValueType::Signed => {
                let sign_bit = 1u64 << (signal.length - 1);
                if raw_value & sign_bit != 0 {
                    // 负数，进行符号扩展
                    let negative_value = raw_value | (!mask);
                    negative_value as i64 as f64
                } else {
                    raw_value as f64
                }
            }
            ValueType::Unsigned => raw_value as f64,
            ValueType::Float => {
                // 简单的浮点转换，实际应用中可能需要更复杂的处理
                raw_value as f64
            }
        };

        // 应用因子和偏移
        Ok(value * signal.factor + signal.offset)
    }

    /// 获取所有消息ID
    pub async fn get_message_ids(&self) -> Vec<u32> {
        let messages = self.messages.read().await;
        messages.keys().cloned().collect()
    }

    /// 获取消息信息
    pub async fn get_message(&self, can_id: u32) -> Option<Message> {
        let messages = self.messages.read().await;
        messages.get(&can_id).cloned()
    }
} 