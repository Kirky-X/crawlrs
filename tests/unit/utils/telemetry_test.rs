// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crawlrs::utils::telemetry;

    #[test]
    fn test_telemetry_initialization() {
        // 初始化遥测系统
        telemetry::init_telemetry();

        // 测试不同级别的日志
        tracing::trace!("This is a trace message");
        tracing::debug!("This is a debug message");
        tracing::info!("This is an info message");
        tracing::warn!("This is a warning message");
        tracing::error!("This is an error message");

        // 测试结构化日志
        tracing::info!(
            user_id = 42,
            user_name = "test_user",
            action = "login",
            "User login successful"
        );

        // 测试错误日志
        let error_result: Result<(), &str> = Err("Test error");
        if let Err(e) = error_result {
            tracing::error!(error = e, "Operation failed");
        }

        // 如果到这里没有panic，说明遥测系统工作正常
        assert!(true);
    }
}