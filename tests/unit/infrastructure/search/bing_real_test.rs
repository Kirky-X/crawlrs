// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crawlrs::infrastructure::search::bing::BingSearchEngine;

/// Test BingSearchEngine with real HTML parsing - using sample data
#[tokio::test]
async fn test_bing_search_engine_real_parsing() {
    let engine = BingSearchEngine::new();

    // Test with real-world HTML structure that Bing actually returns
    let real_bing_html = r#"
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <title>Rust programming - Bing</title>
    </head>
    <body>
        <ol id="b_results">
            <li class="b_algo">
                <h2>
                    <a href="https://www.rust-lang.org/" h="ID=SERP,5006.1">
                        <strong>Rust</strong> Programming Language
                    </a>
                </h2>
                <div class="b_caption">
                    <p>Rust is a systems programming language focused on safety and performance. It prevents segfaults and guarantees thread safety.</p>
                </div>
                <div class="b_attribution">
                    <cite>https://www.rust-lang.org</cite>
                </div>
            </li>
            <li class="b_algo">
                <h2>
                    <a href="https://doc.rust-lang.org/book/" h="ID=SERP,5012.1">
                        The Rust Programming Language - The Rust Programming Language Book
                    </a>
                </h2>
                <div class="b_caption">
                    <p>A comprehensive guide to the Rust programming language, written by the Rust development team.</p>
                </div>
                <div class="b_attribution">
                    <cite>https://doc.rust-lang.org</cite>
                </div>
            </li>
        </ol>
    </body>
    </html>
    "#;

    let result = engine
        .parse_search_results(real_bing_html, "rust programming")
        .await;

    assert!(result.is_ok());
    let results = result.unwrap();
    assert_eq!(results.len(), 2);

    // Verify first result
    assert_eq!(results[0].title, "Rust Programming Language");
    assert_eq!(results[0].url, "https://www.rust-lang.org/");
    assert_eq!(results[0].engine, "bing");
    assert!(results[0].score >= 0.0 && results[0].score <= 1.0);

    // Verify second result
    assert_eq!(
        results[1].title,
        "The Rust Programming Language - The Rust Programming Language Book"
    );
    assert_eq!(results[1].url, "https://doc.rust-lang.org/book/");
    assert_eq!(results[1].engine, "bing");
    assert!(results[1].score >= 0.0 && results[1].score <= 1.0);
}

/// Test BingSearchEngine with real HTML that has malformed elements
#[tokio::test]
async fn test_bing_search_engine_real_malformed_html() {
    let engine = BingSearchEngine::new();

    // Test with real HTML that has missing or malformed elements
    let malformed_html = r#"
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <title>Test query - Bing</title>
    </head>
    <body>
        <ol id="b_results">
            <li class="b_algo">
                <h2></h2>
                <div class="b_caption">
                    <p>Result with empty title</p>
                </div>
            </li>
            <li class="b_algo">
                <h2>
                    <a href="not-a-valid-url">
                        Valid Title
                    </a>
                </h2>
                <div class="b_caption">
                    <p>Result with invalid URL</p>
                </div>
            </li>
            <li class="b_algo">
                <h2>
                    <a href="https://valid.com/article" h="ID=SERP,5015.1">
                        Valid Article Title
                    </a>
                </h2>
                <div class="b_caption">
                    <p>This result should be included and has a publication date.</p>
                </div>
                <div class="b_attribution">
                    <cite>https://valid.com</cite>
                </div>
            </li>
        </ol>
    </body>
    </html>
    "#;

    let result = engine
        .parse_search_results(malformed_html, "test query")
        .await;

    assert!(result.is_ok());
    let results = result.unwrap();

    // Should only include the valid result
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Valid Article Title");
    assert_eq!(results[0].url, "https://valid.com/article");
}

/// Test BingSearchEngine with real HTML containing dates and metadata
#[tokio::test]
async fn test_bing_search_engine_real_html_with_dates() {
    let engine = BingSearchEngine::new();

    // Test with real HTML that contains publication dates
    let html_with_dates = r#"
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <title>Rust tutorial - Bing</title>
    </head>
    <body>
        <ol id="b_results">
            <li class="b_algo">
                <h2>
                    <a href="https://blog.example.com/rust-tutorial" h="ID=SERP,5020.1">
                        Comprehensive Rust Tutorial for Beginners
                    </a>
                </h2>
                <div class="b_caption">
                    <p>Published on Jan 15, 2024 - This comprehensive Rust tutorial covers all the basics you need to get started with systems programming.</p>
                </div>
                <div class="b_attribution">
                    <cite>https://blog.example.com</cite>
                </div>
            </li>
            <li class="b_algo">
                <h2>
                    <a href="https://tutorial.example.com/rust" h="ID=SERP,5025.1">
                        Advanced Rust Programming Techniques
                    </a>
                </h2>
                <div class="b_caption">
                    <p>Last updated: March 2024 - Learn advanced Rust programming techniques including memory management and concurrency.</p>
                </div>
                <div class="b_attribution">
                    <cite>https://tutorial.example.com</cite>
                </div>
            </li>
        </ol>
    </body>
    </html>
    "#;

    let result = engine
        .parse_search_results(html_with_dates, "rust tutorial")
        .await;

    assert!(result.is_ok());
    let results = result.unwrap();

    // Should parse both results correctly
    assert_eq!(results.len(), 2);

    // Verify first result with date
    assert_eq!(
        results[0].title,
        "Comprehensive Rust Tutorial for Beginners"
    );
    assert_eq!(results[0].url, "https://blog.example.com/rust-tutorial");
    assert!(results[0]
        .description
        .as_ref()
        .unwrap()
        .contains("Published on Jan 15, 2024"));

    // Verify second result with date
    assert_eq!(results[1].title, "Advanced Rust Programming Techniques");
    assert_eq!(results[1].url, "https://tutorial.example.com/rust");
    assert!(results[1]
        .description
        .as_ref()
        .unwrap()
        .contains("Last updated: March 2024"));
}

/// Test BingSearchEngine with empty real HTML
#[tokio::test]
async fn test_bing_search_engine_real_empty_html() {
    let engine = BingSearchEngine::new();

    // Test with real empty HTML
    let empty_html = r#"
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <title>Empty results - Bing</title>
    </head>
    <body>
        <div class="no_results">
            <p>No results found for your search.</p>
        </div>
    </body>
    </html>
    "#;

    let result = engine
        .parse_search_results(empty_html, "nonexistent query")
        .await;

    // Should return empty results, not an error
    assert!(result.is_ok());
    let results = result.unwrap();
    assert_eq!(results.len(), 0);
}

/// Test BingSearchEngine cookie and URL building with real parameters
#[tokio::test]
async fn test_bing_search_engine_real_cookie_construction() {
    let engine = BingSearchEngine::new();

    // Test real cookie construction for different locales
    let test_cases = vec![
        ("en", "US"),
        ("zh", "CN"),
        ("ja", "JP"),
        ("de", "DE"),
        ("fr", "FR"),
    ];

    for (lang, region) in test_cases {
        let cookies = engine.get_bing_cookies(lang, region);

        // Verify cookies are constructed correctly
        assert_eq!(
            cookies.get("_EDGE_CD"),
            Some(&format!("m={}&u={}", region, lang))
        );
        assert_eq!(
            cookies.get("_EDGE_S"),
            Some(&format!("mkt={}&ui={}", region, lang))
        );
    }
}

/// Test BingSearchEngine URL building with real pagination
#[tokio::test]
async fn test_bing_search_engine_real_url_construction() {
    let engine = BingSearchEngine::new();

    let query = "rust programming tutorial";

    // Test URL construction for different pages
    let page_urls: Vec<String> = (1..=5)
        .map(|page| engine.build_bing_url(query, page))
        .collect();

    // Verify each URL contains the query
    for url in &page_urls {
        assert!(url.contains("q=rust+programming+tutorial"));
        assert!(url.contains("pq=rust+programming+tutorial"));
    }

    // Verify first page has no pagination parameters
    assert!(!page_urls[0].contains("first="));
    assert!(!page_urls[0].contains("FORM="));

    // Verify subsequent pages have correct pagination
    assert!(page_urls[1].contains("first=11"));
    assert!(page_urls[1].contains("FORM=PERE"));

    assert!(page_urls[2].contains("first=21"));
    assert!(page_urls[2].contains("FORM=PERE1"));

    assert!(page_urls[3].contains("first=31"));
    assert!(page_urls[3].contains("FORM=PERE2"));

    assert!(page_urls[4].contains("first=41"));
    assert!(page_urls[4].contains("FORM=PERE3"));
}
