// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 配置设置测试模块
///
/// 测试配置加载和验证功能
/// 确保配置系统能够正确解析和验证各种配置参数

#[cfg(test)]
mod tests {
    use crawlrs::config::settings::Settings;

    #[test]
    fn test_config_loading_from_default_toml() {
        println!("Testing configuration loading from default.toml...");

        match Settings::new() {
            Ok(settings) => {
                println!("✓ Configuration loaded successfully");
                println!("Google Search Config:");
                println!(
                    "  API Key: {}",
                    if settings.google_search.api_key.is_some() {
                        "[SET]"
                    } else {
                        "[NOT SET]"
                    }
                );
                println!(
                    "  CX: {}",
                    if settings.google_search.cx.is_some() {
                        "[SET]"
                    } else {
                        "[NOT SET]"
                    }
                );

                println!("\nLLM Config:");
                println!(
                    "  API Key: {}",
                    if settings.llm.api_key.is_some() {
                        "[SET]"
                    } else {
                        "[NOT SET]"
                    }
                );
                println!("  Model: {:?}", settings.llm.model);
                println!("  API Base URL: {:?}", settings.llm.api_base_url);

                println!("\nDatabase Config:");
                println!("  URL: {}", settings.database.url);

                println!("\nRedis Config:");
                println!("  URL: {}", settings.redis.url);

                // Verify that the new config sections are present
                assert!(settings.google_search.api_key.is_some());
                assert!(settings.google_search.cx.is_some());
                assert!(settings.llm.api_key.is_some());
                assert!(settings.llm.model.is_some());
                assert!(settings.llm.api_base_url.is_some());

                println!("\n✓ All configuration sections loaded successfully!");
            }
            Err(e) => {
                panic!("✗ Failed to load configuration: {}", e);
            }
        }
    }
}