// Engine validators unit tests
//
// These tests cover the request validation logic.

#[cfg(test)]
mod tests {
    use crawlrs::engines::client::{EngineClient, ScrapeOptions, ScrapeRequest};

    #[tokio::test]
    async fn test_valid_url_request() {
        let request = ScrapeRequest::new("https://example.com");
        assert_eq!(request.url, "https://example.com");
    }

    #[tokio::test]
    async fn test_empty_url_request() {
        let request = ScrapeRequest::new("");
        assert!(request.url.is_empty());
    }

    #[tokio::test]
    async fn test_options_builder() {
        let options = ScrapeOptions::builder()
            .needs_js(true)
            .timeout(std::time::Duration::from_secs(30))
            .build();

        assert!(options.needs_js);
        assert_eq!(options.timeout, std::time::Duration::from_secs(30));
    }
}
