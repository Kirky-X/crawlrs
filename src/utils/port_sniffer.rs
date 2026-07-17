// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use log::{info, warn};
use std::net::TcpListener;
use std::time::Duration;
use thiserror::Error;

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
    /// * `max_attempts` - 最大尝试次数
    ///
    /// # 返回值
    ///
    /// * `Result<PortSnifferResult, PortSnifferError>` - 嗅探结果或错误
    pub fn find_available_port(
        start_port: u16,
        enable_detection: bool,
        max_attempts: u16,
    ) -> Result<PortSnifferResult, PortSnifferError> {
        let mut logs = Vec::new();
        logs.push(format!("开始端口检测，起始端口: {}", start_port));

        // 如果未启用检测功能，直接检查起始端口
        if !enable_detection {
            if Self::is_port_in_use(start_port) {
                logs.push(format!(
                    "端口 {} 已被占用，且自动嗅探功能未启用",
                    start_port
                ));
                warn!("端口 {} 已被占用，且自动嗅探功能未启用", start_port);
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
        let mut attempts = 0;

        while attempts < max_attempts {
            if !Self::is_port_in_use(current_port) {
                logs.push(format!("找到可用端口: {}", current_port));
                info!("找到可用端口: {}", current_port);
                return Ok(PortSnifferResult {
                    success: true,
                    port: current_port,
                    logs,
                });
            }

            attempts += 1;
            logs.push(format!("端口 {} 已被占用", current_port));

            // 只在最后几次尝试时显示警告
            if attempts < max_attempts {
                warn!(
                    "端口 {} 已被占用，尝试下一个端口... ({}/{})",
                    current_port, attempts, max_attempts
                );
            }

            // 检查下一个端口是否超出范围
            if current_port == 65535 {
                return Err(PortSnifferError::PortOutOfRange(current_port));
            }

            current_port += 1;

            // 适当的间隔时间，避免检测过快
            std::thread::sleep(Duration::from_millis(100));
        }

        Err(PortSnifferError::NoAvailablePort(format!(
            "在尝试 {} 个端口后未找到可用端口 (范围 {}-{})",
            max_attempts,
            start_port,
            current_port - 1
        )))
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

        let result = PortSniffer::find_available_port(port, true, 50).unwrap();
        assert_eq!(result.port, port);
        assert!(result.success);
    }

    #[test]
    fn test_find_available_port_with_conflict() {
        // 占用一个端口
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // 尝试从该端口开始查找，应该找到下一个可用端口
        let result = PortSniffer::find_available_port(port, true, 50).unwrap();

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
        let result = PortSniffer::find_available_port(port, false, 50).unwrap();
        assert_eq!(result.port, port);
    }

    // ========== is_port_in_use 边界 ==========

    #[test]
    fn test_is_port_in_use_returns_false_for_free_port() {
        // 绑定并立即释放一个端口
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        // 端口被释放后应不在使用中
        // 注意：在并发环境或 TIME_WAIT 状态下可能存在轻微 flake 风险
        assert!(
            !PortSniffer::is_port_in_use(port),
            "port {} should be free after drop",
            port
        );
    }

    // ========== find_available_port: 日志验证 ==========

    #[test]
    fn test_find_available_port_logs_start_message() {
        // 释放一个端口用作起始
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let result = PortSniffer::find_available_port(port, true, 5).unwrap();
        assert!(
            result
                .logs
                .iter()
                .any(|log| log.contains("开始端口检测") && log.contains(&port.to_string())),
            "logs should contain start message with port, got: {:?}",
            result.logs
        );
    }

    #[test]
    fn test_find_available_port_logs_found_message() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let result = PortSniffer::find_available_port(port, true, 5).unwrap();
        assert!(
            result.logs.iter().any(|log| log.contains("找到可用端口")),
            "logs should contain found message, got: {:?}",
            result.logs
        );
    }

    #[test]
    fn test_find_available_port_disabled_detection_logs_in_use() {
        // 占用一个端口
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let result = PortSniffer::find_available_port(port, false, 5).unwrap();
        // 禁用检测且端口被占用时，应记录 "已被占用，且自动嗅探功能未启用"
        assert!(
            result
                .logs
                .iter()
                .any(|log| log.contains("已被占用") && log.contains("自动嗅探功能未启用")),
            "logs should indicate port in use and detection disabled, got: {:?}",
            result.logs
        );
    }

    #[test]
    fn test_find_available_port_disabled_detection_no_log_when_free() {
        // 释放端口
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let result = PortSniffer::find_available_port(port, false, 5).unwrap();
        // 禁用检测且端口可用时，不应有 "已被占用" 日志
        assert!(
            !result.logs.iter().any(|log| log.contains("已被占用")),
            "should not log in-use when port is free, got: {:?}",
            result.logs
        );
    }

    // ========== find_available_port: success / port 字段 ==========

    #[test]
    fn test_find_available_port_success_is_true() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let result = PortSniffer::find_available_port(port, true, 10).unwrap();
        assert!(result.success, "success should be true when port found");
    }

    #[test]
    fn test_find_available_port_disabled_detection_success_is_true() {
        // 即使端口被占用，禁用检测时 success 仍为 true
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let result = PortSniffer::find_available_port(port, false, 5).unwrap();
        assert!(
            result.success,
            "success should be true even when port is in use with detection disabled"
        );
        assert_eq!(result.port, port);
    }

    #[test]
    fn test_find_available_port_with_conflict_increments_port() {
        // 占用起始端口，检测应找到下一个可用端口
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let result = PortSniffer::find_available_port(port, true, 50).unwrap();
        assert!(
            result.port > port,
            "should find a port greater than occupied start port"
        );
        // 日志应包含 "已被占用" 记录
        assert!(result.logs.iter().any(|log| log.contains("已被占用")));
    }

    // ========== find_available_port: 错误路径 ==========

    #[test]
    fn test_find_available_port_zero_attempts_returns_error() {
        // max_attempts=0 时 while 循环不执行，直接返回 NoAvailablePort
        let result = PortSniffer::find_available_port(8080, true, 0);
        assert!(result.is_err());
        match result {
            Err(PortSnifferError::NoAvailablePort(msg)) => {
                assert!(
                    msg.contains("8080"),
                    "error message should contain start port, got: {}",
                    msg
                );
                assert!(
                    msg.contains("0"),
                    "error message should contain attempts count, got: {}",
                    msg
                );
            }
            other => panic!("expected NoAvailablePort, got {:?}", other),
        }
    }

    #[test]
    fn test_find_available_port_one_attempt_with_conflict_returns_error() {
        // 占用端口，max_attempts=1 时只尝试一次，无法找到下一个
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let result = PortSniffer::find_available_port(port, true, 1);
        assert!(
            result.is_err(),
            "should return error when only 1 attempt and port is busy"
        );
        assert!(
            matches!(result, Err(PortSnifferError::NoAvailablePort(_))),
            "expected NoAvailablePort, got {:?}",
            result
        );
    }

    #[test]
    fn test_find_available_port_at_65535_boundary_with_conflict() {
        // 尝试绑定 65535；若不可绑定则跳过该测试（环境占用）
        let listener = match TcpListener::bind("0.0.0.0:65535") {
            Ok(l) => l,
            Err(_) => {
                // 65535 已被占用，无法测试 PortOutOfRange 路径
                return;
            }
        };

        // 端口 65535 被占用时，应返回 PortOutOfRange(65535)
        let result = PortSniffer::find_available_port(65535, true, 5);
        assert!(result.is_err());
        match result {
            Err(PortSnifferError::PortOutOfRange(port)) => {
                assert_eq!(port, 65535);
            }
            other => panic!("expected PortOutOfRange(65535), got {:?}", other),
        }
        drop(listener);
    }

    // ========== PortSnifferError: Display ==========

    #[test]
    fn test_port_sniffer_error_port_out_of_range_display() {
        // u16 最大值 65535；此处验证 Display 输出包含端口号与范围提示
        let err = PortSnifferError::PortOutOfRange(65535u16);
        let msg = err.to_string();
        assert!(msg.contains("65535"), "msg: {}", msg);
        assert!(msg.contains("超出有效范围"), "msg: {}", msg);
    }

    #[test]
    fn test_port_sniffer_error_no_available_port_display() {
        let err = PortSnifferError::NoAvailablePort("no port found in range".to_string());
        let msg = err.to_string();
        assert!(msg.contains("未找到可用端口"), "msg: {}", msg);
        assert!(msg.contains("no port found in range"), "msg: {}", msg);
    }

    // ========== PortSnifferResult: derive 行为 ==========

    #[test]
    fn test_port_sniffer_result_clone() {
        let result = PortSnifferResult {
            success: true,
            port: 8080,
            logs: vec!["log1".to_string(), "log2".to_string()],
        };
        let cloned = result.clone();
        assert_eq!(cloned.success, result.success);
        assert_eq!(cloned.port, result.port);
        assert_eq!(cloned.logs, result.logs);
    }

    #[test]
    fn test_port_sniffer_result_debug() {
        let result = PortSnifferResult {
            success: false,
            port: 9000,
            logs: vec!["debug-log".to_string()],
        };
        let debug_str = format!("{:?}", result);
        assert!(
            debug_str.contains("PortSnifferResult"),
            "debug: {}",
            debug_str
        );
        assert!(debug_str.contains("9000"), "debug: {}", debug_str);
        assert!(debug_str.contains("debug-log"), "debug: {}", debug_str);
    }

    #[test]
    fn test_port_sniffer_result_construction() {
        let logs = vec!["开始端口检测".to_string(), "找到可用端口".to_string()];
        let result = PortSnifferResult {
            success: true,
            port: 3000,
            logs: logs.clone(),
        };
        assert!(result.success);
        assert_eq!(result.port, 3000);
        assert_eq!(result.logs, logs);
        assert_eq!(result.logs.len(), 2);
    }

    #[test]
    fn test_find_available_port_returns_logs_non_empty() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let result = PortSniffer::find_available_port(port, true, 5).unwrap();
        // 至少应包含开始日志和找到日志
        assert!(!result.logs.is_empty(), "logs should not be empty");
        assert!(
            result.logs.len() >= 2,
            "should have at least start and found logs"
        );
    }
}
