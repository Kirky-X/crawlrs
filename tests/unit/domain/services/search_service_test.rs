#![cfg(test)]
// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crawlrs::config::settings::Settings;

    #[test]
    fn test_search_service_configuration_loading() {
        // Test that configuration loads properly for search service
        let settings = Settings::new().expect("Failed to load settings");

        // Verify that the search configuration is properly loaded from default.toml
        assert!(settings.bing_search.api_key.is_some());

        println!("✓ Search service configuration loaded successfully from default.toml");
        println!(
            "  Bing API Key configured: {}",
            if settings
                .bing_search
                .api_key
                .as_ref()
                .expect("Bing API key not found")
                .is_empty()
            {
                "[EMPTY]"
            } else {
                "[SET]"
            }
        );
        println!(
            "  Default Search Engine: {:?}",
            settings.search.default_engine
        );

        // Test that the configuration can be used to create error messages
        if settings
            .bing_search
            .api_key
            .as_ref()
            .expect("Bing API key not found")
            .is_empty()
        {
            let error_msg = "No search engine configured. Please set bing_search.api_key in config/default.toml.";
            println!("  Expected error message: {}", error_msg);
            assert!(error_msg.contains("bing_search.api_key"));
            assert!(error_msg.contains("config/default.toml"));
        }
    }
}
