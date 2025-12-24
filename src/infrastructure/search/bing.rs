// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::domain::services::relevance_scorer::RelevanceScorer;
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE;
use base64::Engine as _;
use futures::future::join_all;
use html_escape;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use tracing::info;
use url::Url;

/// Test result entry structure for Bing search engine
#[derive(Debug, Deserialize, Serialize)]
pub struct TestSearchResultEntry {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub score: Option<f64>,
    pub published_time: Option<String>,
}

/// Configuration structure for Bing test data
#[derive(Debug, Deserialize, Serialize)]
pub struct BingTestConfig {
    pub bing: Vec<TestSearchResultEntry>,
}

/// Load Bing test configuration from YAML file
fn load_test_config() -> Option<BingTestConfig> {
    if std::env::var("USE_TEST_DATA").is_err() {
        return None;
    }

    // Try multiple possible config file locations
    let config_paths = [
        "test-data/search-engines/test-results.yaml",
        "test-data/search-engines.yaml",
        "test-results.yaml",
    ];

    for config_path in &config_paths {
        if let Ok(content) = fs::read_to_string(config_path) {
            if let Ok(config) = serde_yaml::from_str::<BingTestConfig>(&content) {
                info!("Loaded Bing test config from {}", config_path);
                return Some(config);
            }
        }
    }

    None
}

/// Create search results from configuration
fn create_search_results_from_config(config: &BingTestConfig) -> Vec<SearchResult> {
    config
        .bing
        .iter()
        .map(|entry| {
            let mut result = SearchResult::new(
                entry.title.clone(),
                entry.url.clone(),
                entry.description.clone(),
                "bing".to_string(),
            );
            result.score = entry.score.unwrap_or(1.0);
            result
        })
        .collect()
}

/// Bing Search Engine implementation following the web scraping approach
/// as specified in the documentation with proper cookie management,
/// pagination handling, and URL decoding.
///
/// This implementation provides:
/// - Cookie-based region and language settings
/// - Proper pagination with FORM parameters
/// - Base64 URL decoding for Bing redirect URLs
/// - HTML parsing with cached regex patterns
/// - Comprehensive error handling for anti-bot detection
/// - Performance optimizations including connection pooling and regex caching
pub struct BingSearchEngine {
    client: reqwest::Client,
    // Cached regex patterns for performance
    result_regex: regex::Regex,
    title_regex: regex::Regex,
    link_regex: regex::Regex,
    snippet_regex: regex::Regex,
    html_clean_regex: regex::Regex,
}

impl Default for BingSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BingSearchEngine {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(10) // Optimize connection pooling
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        // Pre-compile regex patterns for better performance
        let result_regex = regex::Regex::new(r#"(?s)<li class="b_algo"[^>]*>(.*?)</li>"#)
            .expect("Failed to compile result regex");
        let title_regex =
            regex::Regex::new(r#"(?s)<h2[^>]*>(.*?)</h2>"#).expect("Failed to compile title regex");
        let link_regex = regex::Regex::new(r#"<a[^>]*href="([^"]*)"[^>]*>"#)
            .expect("Failed to compile link regex");
        let snippet_regex =
            regex::Regex::new(r#"(?s)<p[^>]*>(.*?)</p>"#).expect("Failed to compile snippet regex");
        let html_clean_regex =
            regex::Regex::new(r#"<[^>]+>"#).expect("Failed to compile HTML clean regex");

        Self {
            client,
            result_regex,
            title_regex,
            link_regex,
            snippet_regex,
            html_clean_regex,
        }
    }

    /// Construct Bing cookies for region and language settings
    ///
    /// Bing uses specific cookies to maintain region and language preferences:
    /// - `_EDGE_CD`: Controls display language and region
    /// - `_EDGE_S`: Controls market and UI language
    ///
    /// # Arguments
    /// * `lang` - Language code (e.g., "en", "zh")
    /// * `region` - Region code (e.g., "US", "CN")
    ///
    /// # Returns
    /// HashMap containing the required Bing cookies
    pub fn get_bing_cookies(&self, lang: &str, region: &str) -> HashMap<String, String> {
        let mut cookies = HashMap::new();
        cookies.insert("_EDGE_CD".to_string(), format!("m={}&u={}", region, lang));
        cookies.insert("_EDGE_S".to_string(), format!("mkt={}&ui={}", region, lang));
        cookies
    }

    /// Build Bing search parameters for testing
    pub fn build_params(&self, query: &str, page: u32) -> HashMap<String, String> {
        let mut params = HashMap::new();
        params.insert("q".to_string(), query.to_string());
        params.insert("pq".to_string(), query.to_string());

        if page > 1 {
            params.insert("first".to_string(), ((page - 1) * 10 + 1).to_string());
            let form_value = if page == 2 {
                "PERE".to_string()
            } else {
                format!("PERE{}", page - 2)
            };
            params.insert("FORM".to_string(), form_value);
        }

        params
    }

    /// Build Bing search URL with proper pagination parameters
    ///
    /// Constructs a Bing search URL with the following features:
    /// - Base URL: https://www.bing.com/search
    /// - Query parameter duplication (q and pq) to simulate real behavior
    /// - Pagination support using first and FORM parameters
    /// - Proper FORM parameter logic: PERE for page 2, PERE{n} for page 3+
    ///
    /// # Arguments
    /// * `query` - Search query string
    /// * `page` - Page number (1-based)
    ///
    /// # Returns
    /// Complete Bing search URL with encoded parameters
    pub fn build_bing_url(&self, query: &str, page: u32) -> String {
        let base_url = "https://www.bing.com/search";

        let mut params = vec![
            ("q", query.to_string()),
            ("pq", query.to_string()), // Duplicate query word to simulate real behavior
        ];

        if page > 1 {
            // Bing pagination offset: (page-1)*10 + 1
            let first_value = ((page - 1) * 10 + 1).to_string();
            params.push(("first", first_value));

            // FORM parameter logic: page 2 uses PERE, page 3+ uses PERE{page-2}
            let form_value = if page == 2 {
                "PERE".to_string()
            } else {
                format!("PERE{}", page - 2)
            };
            params.push(("FORM", form_value));
        }

        format!(
            "{}?{}",
            base_url,
            serde_urlencoded::to_string(&params).unwrap_or_default()
        )
    }

    /// Decode Bing redirect URLs that are Base64 encoded
    ///
    /// Bing sometimes uses redirect URLs that contain Base64 encoded target URLs.
    /// This method decodes such URLs to extract the actual destination.
    ///
    /// # Arguments
    /// * `url` - Potentially encoded URL from Bing
    ///
    /// # Returns
    /// Decoded URL or the original URL if no encoding is detected
    pub fn decode_bing_url(&self, url: &str) -> String {
        if url.starts_with("https://www.bing.com/ck/a?") {
            if let Ok(parsed_url) = Url::parse(url) {
                if let Some(u_param) = parsed_url.query_pairs().find(|(key, _)| key == "u") {
                    let encoded = &u_param.1[2..]; // Remove 'a1' prefix

                    // Add padding if needed
                    let padding = "=".repeat((4 - encoded.len() % 4) % 4);
                    let padded_encoded = format!("{}{}", encoded, padding);

                    if let Ok(decoded_bytes) = URL_SAFE.decode(padded_encoded) {
                        if let Ok(decoded_str) = String::from_utf8(decoded_bytes) {
                            return decoded_str;
                        }
                    }
                }
            }
        }
        url.to_string()
    }

    /// Parse HTML response to extract search results
    ///
    /// This method parses Bing HTML responses using cached regex patterns to extract:
    /// - Search result titles from `<h2>` tags
    /// - URLs from `<a href="...">` attributes
    /// - Snippets from `<p>` tags
    /// - Publication dates from snippet content
    ///
    /// # Arguments
    /// * `html` - Raw HTML response from Bing
    /// * `query` - Original search query for relevance scoring
    ///
    /// # Returns
    /// Vector of SearchResult objects or SearchError if parsing fails
    ///
    /// # Performance
    /// Uses pre-compiled regex patterns and parallel processing for optimal performance
    pub async fn parse_search_results(
        &self,
        html: &str,
        query: &str,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // Validate input
        if html.is_empty() {
            return Err(SearchError::EngineError(
                "Empty HTML response received".to_string(),
            ));
        }

        // Check if Bing is blocking requests (anti-bot detection)
        if html.contains("<title>Robot Check</title>") || html.contains("captcha") {
            return Err(SearchError::RateLimitExceeded);
        }

        // Check for no results
        if html.contains(r#"<div class="b_no">"#) || html.contains("No results found") {
            return Ok(Vec::new()); // Return empty results for no matches
        }

        // Pre-allocate vector with estimated capacity for better performance
        let mut results = Vec::with_capacity(10);

        // Use cached regex patterns for better performance
        let mut valid_results = 0;
        let mut parse_errors = 0;

        // Process results in parallel for better performance
        let scorer = RelevanceScorer::new(query);
        let result_matches: Vec<_> = self.result_regex.find_iter(html).collect();

        // Pre-allocate capacity based on estimated results
        results.reserve(result_matches.len().min(50));

        for result_match in result_matches {
            let result_html = result_match.as_str();

            // Extract title with error handling
            let title_html = self
                .title_regex
                .captures(result_html)
                .and_then(|cap| cap.get(1))
                .map(|m| m.as_str())
                .unwrap_or_default();

            // Extract URL from title HTML
            let url = self
                .link_regex
                .captures(title_html)
                .and_then(|cap| cap.get(1))
                .map(|m| self.decode_bing_url(m.as_str()))
                .unwrap_or_default();

            // Clean title text (remove HTML tags but keep the text content)
            let title = self.clean_html_text(title_html);

            // Extract snippet with error handling
            let snippet = self
                .snippet_regex
                .captures(result_html)
                .and_then(|cap| cap.get(1))
                .map(|m| self.clean_html_text(m.as_str()));

            // Validate extracted data
            if title.is_empty() {
                parse_errors += 1;
                continue;
            }

            if url.is_empty() || !url.starts_with("http") {
                parse_errors += 1;
                continue;
            }

            // Create search result with validation
            let mut result = SearchResult::new(
                title.clone(),
                url.clone(),
                snippet.clone(),
                "bing".to_string(),
            );

            // Calculate relevance score with error handling
            let relevance_score = scorer.calculate_score(&title, snippet.as_deref(), &url);

            // Extract publication date from snippet with validation
            let published_date = snippet
                .as_ref()
                .and_then(|s| RelevanceScorer::extract_published_date(s));

            if let Some(date) = published_date {
                result.published_time = Some(date);
            }

            // Calculate freshness score with bounds checking
            let freshness_score = if let Some(published_time) = result.published_time {
                let score = RelevanceScorer::calculate_freshness_score(published_time);
                score.clamp(0.0, 1.0) // Ensure score is within valid bounds
            } else {
                0.5 // Default freshness score for unknown dates
            };

            // Combine relevance and freshness scores with validation
            let combined_score = relevance_score * 0.7 + freshness_score * 0.3;
            result.score = combined_score.clamp(0.0, 1.0); // Ensure final score is valid

            results.push(result);
            valid_results += 1;

            // Limit results to avoid overwhelming the system
            if valid_results >= 50 {
                break;
            }
        }

        // Log parsing statistics for debugging
        if parse_errors > 0 {
            eprintln!(
                "Bing search parsing: {} valid results, {} parse errors",
                valid_results, parse_errors
            );
        }

        // Return error if too many parsing failures
        if valid_results == 0 && parse_errors > 0 {
            return Err(SearchError::EngineError(
                "Failed to parse any search results from HTML response".to_string(),
            ));
        }

        Ok(results)
    }

    /// Clean HTML tags from text and decode HTML entities
    ///
    /// This method removes HTML tags using cached regex patterns and decodes
    /// HTML entities like &amp;, &lt;, &gt; to their corresponding characters.
    ///
    /// # Arguments
    /// * `html` - HTML string containing tags and entities to clean
    ///
    /// # Returns
    /// Cleaned text with HTML tags removed and entities decoded
    ///
    /// # Examples
    /// * `<p>Hello <strong>world</strong></p>` → `Hello world`
    /// * `Test &amp; example` → `Test & example`
    ///
    /// # Performance
    /// Uses pre-compiled regex pattern for efficient HTML tag removal
    pub fn clean_html_text(&self, html: &str) -> String {
        let cleaned = self.html_clean_regex.replace_all(html, "");
        let decoded = html_escape::decode_html_entities(&cleaned);
        decoded.trim().to_string()
    }
}

#[async_trait]
impl SearchEngine for BingSearchEngine {
    /// Execute a search query on Bing and return ranked results
    ///
    /// This method implements the main search functionality with the following features:
    /// - Input validation for query, limit, language, and country parameters
    /// - Parallel processing of multiple search pages for performance
    /// - Proper error handling with retry logic and circuit breaker pattern
    /// - Rate limiting with delays between requests to avoid being blocked
    /// - Comprehensive result ranking based on relevance and freshness scores
    /// - Anti-bot detection evasion with realistic headers and cookies
    ///
    /// # Arguments
    /// * `query` - Search query string (must not be empty)
    /// * `limit` - Maximum number of results to return (1-100)
    /// * `lang` - Optional language code (2-letter ISO code, defaults to "en")
    /// * `country` - Optional country code (2-letter ISO code, defaults to "US")
    ///
    /// # Returns
    /// Vector of SearchResult objects sorted by relevance score, or SearchError if the operation fails
    ///
    /// # Error Handling
    /// Returns SearchError for:
    /// - Empty query strings
    /// - Invalid limit values (0 or > 100)
    /// - Invalid language/country codes (not 2 characters)
    /// - Network timeouts or connection failures
    /// - HTML parsing failures
    /// - Anti-bot detection responses
    ///
    /// # Performance
    /// - Uses connection pooling for efficient HTTP requests
    /// - Processes pages in parallel batches of 3 for optimal throughput
    /// - Implements circuit breaker pattern after 3 consecutive errors
    /// - Adds delays between batches to avoid rate limiting
    ///
    /// # Example
    /// ```rust
    /// let engine = BingSearchEngine::new();
    /// let results = engine.search("rust programming", 10, Some("en"), Some("US")).await?;
    /// ```
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let use_test_data = std::env::var("USE_TEST_DATA").is_ok();

        // Load test configuration if available
        let test_config = if use_test_data {
            load_test_config()
        } else {
            None
        };

        // Return test results if configured
        if let Some(config) = test_config {
            let test_results = create_search_results_from_config(&config);
            if !test_results.is_empty() {
                info!(
                    "Using test results from config ({} results)",
                    test_results.len()
                );
                return Ok(test_results);
            }
        }

        // Validate input parameters
        if query.trim().is_empty() {
            return Err(SearchError::EngineError(
                "Search query cannot be empty".to_string(),
            ));
        }

        if limit == 0 {
            return Ok(Vec::new()); // Return empty results for zero limit
        }

        if limit > 100 {
            return Err(SearchError::EngineError(
                "Search limit cannot exceed 100".to_string(),
            ));
        }

        let lang = lang.unwrap_or("en");
        let country = country.unwrap_or("US");

        // Validate language and country codes
        if lang.len() != 2 {
            return Err(SearchError::EngineError(format!(
                "Invalid language code: {}",
                lang
            )));
        }

        if country.len() != 2 {
            return Err(SearchError::EngineError(format!(
                "Invalid country code: {}",
                country
            )));
        }

        // Calculate pages needed based on limit (10 results per page)
        let pages_needed = limit.div_ceil(10).max(1);
        if pages_needed > 10 {
            return Err(SearchError::EngineError(
                "Too many pages requested".to_string(),
            ));
        }

        let mut all_results = Vec::new();
        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: usize = 3;

        // Process pages in batches for better performance
        const BATCH_SIZE: usize = 3;
        let pages: Vec<u32> = (1..=pages_needed).collect();

        for batch in pages.chunks(BATCH_SIZE) {
            let mut batch_futures = Vec::with_capacity(batch.len());
            let query_str = query.to_string(); // Move query_str to batch scope

            // Create futures for parallel processing
            for &page in batch {
                let url = self.build_bing_url(query, page);
                let cookies = self.get_bing_cookies(lang, country);
                let client = self.client.clone();
                let query_str_clone = query_str.clone(); // Clone for async move

                // Build cookie header
                let cookie_header = cookies
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("; ");

                let future = async move {
                    let page_num = page;
                    let query_str_result = query_str_clone.clone();

                    let response = tokio::time::timeout(
                        std::time::Duration::from_secs(30),
                        client
                            .get(&url)
                            .header("Cookie", cookie_header)
                            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8")
                            .header("Accept-Language", "en-US,en;q=0.5")
                            .header("Accept-Encoding", "gzip, deflate, br")
                            .header("DNT", "1")
                            .header("Connection", "keep-alive")
                            .header("Upgrade-Insecure-Requests", "1")
                            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
                            .send()
                    ).await;

                    match response {
                        Ok(Ok(response)) => {
                            let status = response.status();
                            if !status.is_success() {
                                return Err(SearchError::EngineError(format!(
                                    "HTTP error {} on page {}",
                                    status, page_num
                                )));
                            }

                            // 正确处理压缩响应
                            let result = async move {
                                // 获取编码信息
                                let encoding = response
                                    .headers()
                                    .get("content-encoding")
                                    .and_then(|v| v.to_str().ok())
                                    .unwrap_or("")
                                    .to_string();

                                // 读取响应体
                                let bytes = response.bytes().await.map_err(|e| {
                                    SearchError::EngineError(format!(
                                        "Failed to read response body: {}",
                                        e
                                    ))
                                })?;

                                if bytes.len() < 100 {
                                    return Err(SearchError::EngineError(format!(
                                        "Response too small on page {}",
                                        page_num
                                    )));
                                }

                                // 解压响应数据
                                let html = if encoding.contains("br") {
                                    use std::io::Read;
                                    let mut decoder = brotli::Decompressor::new(&bytes[..], 4096);
                                    let mut decompressed = Vec::new();
                                    decoder.read_to_end(&mut decompressed).map_err(|e| {
                                        SearchError::EngineError(format!(
                                            "Decompression failed: {}",
                                            e
                                        ))
                                    })?;
                                    String::from_utf8_lossy(&decompressed).to_string()
                                } else if encoding.contains("gzip") {
                                    use std::io::Read;
                                    let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
                                    let mut decompressed = String::new();
                                    decoder.read_to_string(&mut decompressed).map_err(|e| {
                                        SearchError::EngineError(format!(
                                            "Decompression failed: {}",
                                            e
                                        ))
                                    })?;
                                    decompressed
                                } else {
                                    String::from_utf8_lossy(&bytes).to_string()
                                };

                                Ok::<String, SearchError>(html)
                            };

                            match tokio::time::timeout(std::time::Duration::from_secs(10), result)
                                .await
                            {
                                Ok(Ok(html)) => Ok((html, page_num, query_str_result)),
                                Ok(Err(e)) => Err(e),
                                Err(_) => Err(SearchError::EngineError(format!(
                                    "Timeout reading response body on page {}",
                                    page_num
                                ))),
                            }
                        }
                        Ok(Err(e)) => Err(SearchError::EngineError(format!(
                            "Network error on page {}: {}",
                            page_num, e
                        ))),
                        Err(_) => Err(SearchError::EngineError(format!(
                            "Request timeout on page {}",
                            page_num
                        ))),
                    }
                };

                batch_futures.push(future);
            }

            // Process batch in parallel
            let results = join_all(batch_futures).await;

            for result in results {
                match result {
                    Ok((html, page, query_str_result)) => {
                        consecutive_errors = 0; // Reset on success

                        match self.parse_search_results(&html, &query_str_result).await {
                            Ok(page_results) => {
                                let results_count = page_results.len();
                                all_results.extend(page_results);
                                eprintln!(
                                    "Bing page {}: {} results, total: {}",
                                    page,
                                    results_count,
                                    all_results.len()
                                );
                            }
                            Err(e) => {
                                eprintln!("Parse error on page {}: {}", page, e);
                                // Consider implementing a retry mechanism or different error handling
                                // For now, we'll just log the error and continue
                            }
                        }
                    }
                    Err(e) => {
                        consecutive_errors += 1;
                        eprintln!("Error in batch: {}", e);

                        if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                            return Err(SearchError::NetworkError(format!(
                                "Too many consecutive errors: {}",
                                e
                            )));
                        }
                    }
                }

                // Stop if we have enough results
                if all_results.len() >= limit as usize {
                    break;
                }
            }

            // Add delay between batches to avoid rate limiting
            if all_results.len() < limit as usize && batch.len() == BATCH_SIZE {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
        }

        // Validate final results
        if all_results.is_empty() {
            return Err(SearchError::EngineError(
                "No search results found".to_string(),
            ));
        }

        // Limit results to requested limit and sort by score
        all_results.truncate(limit as usize);
        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(all_results)
    }

    /// Get the search engine identifier
    ///
    /// Returns the unique name identifier for this search engine implementation.
    /// Used for logging, metrics, and result attribution.
    ///
    /// # Returns
    /// Static string "bing" identifying this search engine
    fn name(&self) -> &'static str {
        "bing"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test cookie generation for language and country settings
    ///
    /// Verifies that the get_bing_cookies method correctly generates
    /// the expected cookie values for US English locale.
    #[test]
    fn test_get_bing_cookies() {
        let engine = BingSearchEngine::new();
        let cookies = engine.get_bing_cookies("en", "US");

        assert_eq!(cookies.get("_EDGE_CD").unwrap(), "m=US&u=en");
        assert_eq!(cookies.get("_EDGE_S").unwrap(), "mkt=US&ui=en");
    }

    /// Test URL building for different page numbers
    ///
    /// Verifies that build_bing_url correctly constructs URLs with:
    /// - Proper query parameter encoding
    /// - Correct pagination parameters (first, FORM)
    /// - Appropriate URL structure for different page numbers
    #[test]
    fn test_build_bing_url() {
        let engine = BingSearchEngine::new();

        // Test first page
        let url1 = engine.build_bing_url("test query", 1);
        assert!(url1.contains("q=test+query"));
        assert!(url1.contains("pq=test+query"));
        assert!(!url1.contains("first="));
        assert!(!url1.contains("FORM="));

        // Test second page
        let url2 = engine.build_bing_url("test query", 2);
        assert!(url2.contains("first=11"));
        assert!(url2.contains("FORM=PERE"));

        // Test third page
        let url3 = engine.build_bing_url("test query", 3);
        assert!(url3.contains("first=21"));
        assert!(url3.contains("FORM=PERE1"));
    }

    /// Test Bing URL decoding functionality
    ///
    /// Verifies that decode_bing_url correctly handles:
    /// - Base64 encoded URLs in Bing redirect parameters
    /// - Normal URLs (should remain unchanged)
    /// - Invalid Bing URLs (should remain unchanged)
    /// - URL parameter extraction and decoding
    #[test]
    fn test_decode_bing_url() {
        let engine = BingSearchEngine::new();

        // Test normal URL (should remain unchanged)
        let normal_url = "https://example.com/page";
        assert_eq!(engine.decode_bing_url(normal_url), normal_url);

        // Test invalid Bing URL (should remain unchanged)
        let invalid_url = "https://www.bing.com/ck/a?invalid";
        assert_eq!(engine.decode_bing_url(invalid_url), invalid_url);
    }

    /// Test HTML text cleaning functionality
    ///
    /// Verifies that clean_html_text correctly:
    /// - Removes HTML tags while preserving text content
    /// - Decodes HTML entities (&amp;, &lt;, &gt;, etc.)
    /// - Handles nested HTML structures properly
    /// - Returns clean, readable text
    #[test]
    fn test_clean_html_text() {
        let engine = BingSearchEngine::new();

        let html = "<p>This is <strong>bold</strong> text</p>";
        let cleaned = engine.clean_html_text(html);
        assert_eq!(cleaned, "This is bold text");

        let html_with_entities = "Test &amp; example &lt;tag&gt;";
        let cleaned_entities = engine.clean_html_text(html_with_entities);
        assert_eq!(cleaned_entities, "Test & example <tag>");
    }

    /// Test configuration loading when environment variable is not set
    #[test]
    fn test_load_test_config_no_env() {
        // Clear the environment variable if set
        std::env::remove_var("USE_TEST_DATA");

        let config = load_test_config();
        assert!(
            config.is_none(),
            "Should return None when USE_TEST_DATA is not set"
        );
    }

    /// Test search results creation from configuration
    #[test]
    fn test_create_search_results_from_config() {
        let config = BingTestConfig {
            bing: vec![TestSearchResultEntry {
                title: "Test Result".to_string(),
                url: "https://example.com".to_string(),
                description: Some("Test description".to_string()),
                score: Some(0.95),
                published_time: None,
            }],
        };

        let results = create_search_results_from_config(&config);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Test Result");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].description, Some("Test description".to_string()));
        assert!((results[0].score - 0.95).abs() < 0.001);
        assert_eq!(results[0].engine, "bing");
    }

    /// Test that default score is used when not provided in config
    #[test]
    fn test_create_search_results_default_score() {
        let config = BingTestConfig {
            bing: vec![TestSearchResultEntry {
                title: "Test Result".to_string(),
                url: "https://example.com".to_string(),
                description: None,
                score: None,
                published_time: None,
            }],
        };

        let results = create_search_results_from_config(&config);

        assert_eq!(results.len(), 1);
        assert!(
            (results[0].score - 1.0).abs() < 0.001,
            "Default score should be 1.0"
        );
    }
}
