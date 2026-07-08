// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crawlrs::utils::telemetry;

    #[test]
    fn test_telemetry_initialization() {
        // 初始化遥测系统
        telemetry::init_telemetry();

        // 测试不同级别的日志
        log::trace!("This is a trace message");
        log::debug!("This is a debug message");
        log::info!("This is an info message");
        log::warn!("This is a warning message");
        log::error!("This is an error message");

        // 测试结构化日志
        log::info!(
            "User login successful: user_id={}, user_name={}, action={}",
            42, "test_user", "login"
        );

        // 测试错误日志
        let error_result: Result<(), &str> = Err("Test error");
        if let Err(e) = error_result {
            log::error!("Operation failed: error={}", e);
        }

        // 如果到这里没有panic，说明遥测系统工作正常
        assert!(true);
    }
}