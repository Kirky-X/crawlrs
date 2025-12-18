// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::net::TcpListener;
use std::time::Duration;
use thiserror::Error;
use tracing::{info, warn};

/// 端口嗅探错误类型
#[derive(Error, Debug)]
pub enum PortSnifferError {
    #[error("端口号 {0} 超出有效范围 (0-65535)")]
    PortOutOfRange(u16),
    #[error("未找到可用端口: {0}")]
    NoAvailablePort(String),
}

/// 端口嗅探结果
#[derive(Debug, Clone)]
pub struct PortSnifferResult {
    /// 是否成功找到端口
    pub success: bool,
    /// 最终使用的端口号
    pub port: u16,
    /// 检测过程日志
    pub logs: Vec<String>,
}

/// 端口嗅探器
pub struct PortSniffer;

impl PortSniffer {
    /// 检查指定端口是否已被占用
    ///
    /// # 参数
    ///
    /// * `port` - 要检查的端口号
    ///
    /// # 返回值
    ///
    /// * `bool` - 如果端口已被占用返回 true，否则返回 false
    pub fn is_port_in_use(port: u16) -> bool {
        TcpListener::bind(("0.0.0.0", port)).is_err()
    }

    /// 查找可用端口
    ///
    /// 如果指定端口被占用，则自动尝试后续端口，直到找到可用端口或达到最大尝试次数
    ///
    /// # 参数
    ///
    /// * `start_port` - 起始端口号
    /// * `enable_detection` - 是否启用自动嗅探功能
    ///
    /// # 返回值
    ///
    /// * `Result<PortSnifferResult, PortSnifferError>` - 嗅探结果或错误
    pub fn find_available_port(start_port: u16, enable_detection: bool) -> Result<PortSnifferResult, PortSnifferError> {
        let mut logs = Vec::new();
        logs.push(format!("开始端口检测，起始端口: {}", start_port));

        // 如果未启用检测功能，直接检查起始端口
        if !enable_detection {
            if Self::is_port_in_use(start_port) {
                logs.push(format!("端口 {} 已被占用，且自动嗅探功能未启用", start_port));
                warn!("端口 {} 已被占用，且自动嗅探功能未启用", start_port);
                // 虽然被占用，但根据需求，如果不启用检测，可能应该直接返回配置的端口让系统去报错，
                // 或者在这里就报错。根据题目要求 "当设置为false时，程序直接使用配置的默认端口而不进行嗅探"，
                // 这意味着我们应该直接返回该端口，让后续流程处理绑定失败的情况，或者在这里返回成功但注明端口。
                // 但为了保持API一致性，这里我们返回该端口，但在日志中记录。
                return Ok(PortSnifferResult {
                    success: true,
                    port: start_port,
                    logs,
                });
            }
            return Ok(PortSnifferResult {
                success: true,
                port: start_port,
                logs,
            });
        }

        let mut current_port = start_port;
        // 设置最大尝试范围，例如尝试100个端口
        let max_port = std::cmp::min(start_port as u32 + 100, 65535) as u16;

        while current_port <= max_port {
            if !Self::is_port_in_use(current_port) {
                logs.push(format!("找到可用端口: {}", current_port));
                info!("找到可用端口: {}", current_port);
                return Ok(PortSnifferResult {
                    success: true,
                    port: current_port,
                    logs,
                });
            }

            logs.push(format!("端口 {} 已被占用", current_port));
            warn!("端口 {} 已被占用，尝试下一个端口...", current_port);
            
            // 检查下一个端口是否超出范围
            if current_port == 65535 {
                return Err(PortSnifferError::PortOutOfRange(current_port));
            }
            
            current_port += 1;
            
            // 适当的间隔时间，避免检测过快
            std::thread::sleep(Duration::from_millis(100));
        }

        Err(PortSnifferError::NoAvailablePort(format!("在范围 {}-{} 内未找到可用端口", start_port, max_port)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn test_is_port_in_use() {
        // 绑定一个随机端口
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        
        // 该端口应该显示被占用
        // 注意：is_port_in_use 尝试绑定 0.0.0.0，如果测试环境支持双栈或特定绑定可能会有差异
        // 这里简单测试逻辑
        assert!(PortSniffer::is_port_in_use(port));
    }

    #[test]
    fn test_find_available_port_no_conflict() {
        // 找一个可用端口
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // 释放该端口
        drop(listener);
        
        let result = PortSniffer::find_available_port(port, true).unwrap();
        assert_eq!(result.port, port);
        assert!(result.success);
    }

    #[test]
    fn test_find_available_port_with_conflict() {
        // 占用一个端口
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        
        // 尝试从该端口开始查找，应该找到下一个可用端口
        let result = PortSniffer::find_available_port(port, true).unwrap();
        
        assert!(result.port > port);
        assert!(result.success);
        assert!(result.logs.iter().any(|log| log.contains("已被占用")));
    }
    
    #[test]
    fn test_disable_detection() {
        // 占用一个端口
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        
        // 禁用检测，应该直接返回原端口
        let result = PortSniffer::find_available_port(port, false).unwrap();
        assert_eq!(result.port, port);
    }
}
