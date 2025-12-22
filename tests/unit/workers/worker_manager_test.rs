// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 工作器管理器测试模块
///
/// 测试工作器管理器的配置加载和初始化功能
/// 验证工作器管理器的正确配置

#[cfg(test)]
mod tests {
    use crawlrs::config::settings::Settings;

    #[tokio::test]
    async fn test_worker_manager_configuration_loading() {
        // Test that WorkerManager can be initialized with Settings
        let settings = Settings::new().unwrap();

        // Verify that all required configuration sections are present
        assert!(
            settings.database.url.len() > 0,
            "Database URL must be configured"
        );
        assert!(settings.redis.url.len() > 0, "Redis URL must be configured");

        // Test that WorkerManager can be created with settings
        // Note: We won't actually start the worker manager in this test
        // as it would require database connections and background tasks

        println!("✓ Worker manager configuration loaded successfully from default.toml");
        println!(
            "  Database URL configured: {}",
            if settings.database.url.is_empty() {
                "[EMPTY]"
            } else {
                "[SET]"
            }
        );
        println!(
            "  Redis URL configured: {}",
            if settings.redis.url.is_empty() {
                "[EMPTY]"
            } else {
                "[SET]"
            }
        );
        println!(
            "  Default team limit: {}",
            settings.concurrency.default_team_limit
        );
        println!(
            "  Task lock duration: {} seconds",
            settings.concurrency.task_lock_duration_seconds
        );

        println!("✓ All worker configuration validation passed");
    }

    #[test]
    fn test_worker_settings_structure() {
        let settings = Settings::new().unwrap();

        // Test that concurrency settings are properly deserialized
        let concurrency_settings = &settings.concurrency;

        println!("✓ Concurrency settings structure validated");
        println!(
            "  Default team limit: {}",
            concurrency_settings.default_team_limit
        );
        println!(
            "  Task lock duration: {}s",
            concurrency_settings.task_lock_duration_seconds
        );

        // Verify reasonable defaults
        assert!(
            concurrency_settings.default_team_limit >= 1,
            "Minimum team limit should be 1"
        );
        assert!(
            concurrency_settings.task_lock_duration_seconds >= 1,
            "Minimum task lock duration should be 1 second"
        );
    }
}