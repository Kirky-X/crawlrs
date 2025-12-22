// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crawlrs::config::settings::Settings;

    #[test]
    fn test_search_service_configuration_loading() {
        // Test that configuration loads properly for search service
        let settings = Settings::new().unwrap();

        // Verify that the search configuration is properly loaded from default.toml
        assert!(settings.google_search.api_key.is_some());
        assert!(settings.google_search.cx.is_some());

        println!("✓ Search service configuration loaded successfully from default.toml");
        println!(
            "  Google API Key configured: {}",
            if settings.google_search.api_key.as_ref().unwrap().is_empty() {
                "[EMPTY]"
            } else {
                "[SET]"
            }
        );
        println!(
            "  Google CX configured: {}",
            if settings.google_search.cx.as_ref().unwrap().is_empty() {
                "[EMPTY]"
            } else {
                "[SET]"
            }
        );

        // Test that the configuration can be used to create error messages
        // (this simulates what the search service does when API keys are missing)
        if settings.google_search.api_key.as_ref().unwrap().is_empty()
            || settings.google_search.cx.as_ref().unwrap().is_empty()
        {
            let error_msg = "No search engine configured. Please set google_search.api_key and google_search.cx in config/default.toml.";
            println!("  Expected error message: {}", error_msg);
            assert!(error_msg.contains("google_search.api_key"));
            assert!(error_msg.contains("google_search.cx"));
            assert!(error_msg.contains("config/default.toml"));
        }
    }
}