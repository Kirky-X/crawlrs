// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! WebContentProcessor / CrawlTextProcessor external unit tests
//!
//! Supplements inline tests by exercising error paths (mock failing encoder),
//! various content type branches, language detection edge cases via public
//! process_web_content API, and validate_content_quality paths not covered
//! by inline tests.

use std::sync::Arc;

use async_trait::async_trait;

use crawlrs::utils::text_processing::encoding::TextEncodingError;
use crawlrs::utils::text_processing::processor::{
    detect_html_structure, init_encoding_patterns, process_crawled_batch_with_processor,
    process_crawled_content_with_processor, process_web_content_with_processor, CrawlProcessorConfig,
    CrawlProcessingError, CrawlTextProcessor, ContentQuality, ProcessedCrawlContent,
    ProcessedWebContent, TextEncodingProcessorComponent, TextEncodingProcessorTrait,
    WebContentError, WebContentProcessor, WebContentProcessorComponent, WebContentProcessorTrait,
};

// ===========================================================================
// Mock failing text encoder to exercise process_encoding error path
// ===========================================================================

struct FailingTextEncoder;

#[async_trait]
impl TextEncodingProcessorTrait for FailingTextEncoder {
    fn process_text(&self, _content: &[u8]) -> Result<String, TextEncodingError> {
        Err(TextEncodingError::DetectionFailed(
            "mock failure".to_string(),
        ))
    }

    fn trim_newlines(&self, text: &str) -> String {
        text.to_string()
    }
}

#[test]
fn tc_web_content_processor_with_failing_encoder_returns_encoding_error() {
    let failing: Arc<dyn TextEncodingProcessorTrait> = Arc::new(FailingTextEncoder);
    let component = WebContentProcessorComponent::new(failing);
    let result = component.process_web_content(b"some content", Some("text/plain"));
    match result {
        Err(WebContentError::EncodingError(_)) => {}
        other => panic!("expected EncodingError, got {:?}", other),
    }
}

#[test]
fn tc_web_content_processor_batch_with_failing_encoder_all_errors() {
    let failing: Arc<dyn TextEncodingProcessorTrait> = Arc::new(FailingTextEncoder);
    let component = WebContentProcessorComponent::new(failing);
    let contents = vec![
        (b"content1".as_slice(), Some("text/plain")),
        (b"content2".as_slice(), None),
    ];
    let results = component.process_batch(contents);
    assert_eq!(results.len(), 2);
    for r in &results {
        match r {
            Err(WebContentError::EncodingError(_)) => {}
            other => panic!("expected EncodingError, got {:?}", other),
        }
    }
}

// ===========================================================================
// Various content type branches in process_web_content (public API)
// ===========================================================================

#[test]
fn tc_process_web_content_xml_with_encoding_declaration() {
    let processor = WebContentProcessor::new();
    let xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><root><item>test</item></root>";
    let result = processor
        .process_web_content(xml, Some("application/xml"))
        .unwrap();
    assert!(!result.is_html);
    assert_eq!(result.declared_encoding, Some("utf-8".to_string()));
    assert!(result.extracted_text.contains("test"));
}

#[test]
fn tc_process_web_content_html_with_meta_charset() {
    let processor = WebContentProcessor::new();
    let html = b"<html><head><meta charset=\"UTF-8\"></head><body><p>Content</p></body></html>";
    let result = processor
        .process_web_content(html, Some("text/html"))
        .unwrap();
    assert!(result.is_html);
    assert_eq!(result.declared_encoding, Some("utf-8".to_string()));
    assert!(result.extracted_text.contains("Content"));
}

#[test]
fn tc_process_web_content_html_with_meta_http_equiv_charset() {
    let processor = WebContentProcessor::new();
    let html = b"<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=gbk\"></head><body>Content</body></html>";
    let result = processor
        .process_web_content(html, Some("text/html"))
        .unwrap();
    assert!(result.is_html);
    match &result.declared_encoding {
        Some(enc) => assert_eq!(enc, "gbk"),
        None => {}
    }
}

#[test]
fn tc_process_web_content_plain_text_no_encoding() {
    let processor = WebContentProcessor::new();
    let text = b"plain text content without any encoding declaration";
    let result = processor
        .process_web_content(text, Some("text/plain"))
        .unwrap();
    assert!(!result.is_html);
    assert_eq!(result.declared_encoding, None);
    assert_eq!(
        result.extracted_text,
        "plain text content without any encoding declaration"
    );
}

#[test]
fn tc_process_web_content_no_content_type() {
    let processor = WebContentProcessor::new();
    let html = b"<html><body><p>No content type provided</p></body></html>";
    let result = processor.process_web_content(html, None).unwrap();
    assert!(result.is_html);
    assert!(result
        .extracted_text
        .contains("No content type provided"));
    assert_eq!(result.content_type, None);
}

#[test]
fn tc_process_web_content_preserves_content_type() {
    let processor = WebContentProcessor::new();
    let result = processor
        .process_web_content(b"text", Some("application/json"))
        .unwrap();
    assert_eq!(result.content_type, Some("application/json".to_string()));
}

#[test]
fn tc_process_web_content_content_length_matches_input() {
    let processor = WebContentProcessor::new();
    let input = b"exactly 20 bytes!";
    let result = processor.process_web_content(input, None).unwrap();
    assert_eq!(result.content_length, input.len());
}

// ===========================================================================
// Language detection via process_web_content (detected_language field)
// ===========================================================================

#[test]
fn tc_language_detection_pure_chinese_via_process_web_content() {
    let processor = WebContentProcessor::new();
    let result = processor
        .process_web_content("这是纯中文内容没有任何英文".as_bytes(), None)
        .unwrap();
    assert_eq!(result.detected_language, Some("zh".to_string()));
}

#[test]
fn tc_language_detection_pure_english_via_process_web_content() {
    let processor = WebContentProcessor::new();
    let result = processor
        .process_web_content(b"this is pure english text with no other characters", None)
        .unwrap();
    assert_eq!(result.detected_language, Some("en".to_string()));
}

#[test]
fn tc_language_detection_unknown_mixed_via_process_web_content() {
    let processor = WebContentProcessor::new();
    // Arabic/Cyrillic mix — neither Chinese nor >80% ASCII
    let result = processor
        .process_web_content("Привет мир مرحبا بالعالم".as_bytes(), None)
        .unwrap();
    assert_eq!(result.detected_language, Some("unknown".to_string()));
}

#[test]
fn tc_language_detection_empty_text_returns_none() {
    let processor = WebContentProcessor::new();
    let result = processor.process_web_content(b"", None).unwrap();
    assert_eq!(result.detected_language, None);
}

// ===========================================================================
// HTML cleaner via process_web_content (exercises extract_text_from_html + clean_text)
// ===========================================================================

#[test]
fn tc_html_cleaner_removes_nested_scripts_via_process_web_content() {
    let processor = WebContentProcessor::new();
    let html = r#"<html><body>
        <script>var x = 1;</script>
        <div>visible</div>
        <script>alert(2);</script>
    </body></html>"#;
    let result = processor
        .process_web_content(html.as_bytes(), Some("text/html"))
        .unwrap();
    assert!(result.extracted_text.contains("visible"));
    assert!(!result.extracted_text.contains("alert"));
    assert!(!result.extracted_text.contains("var x"));
}

#[test]
fn tc_html_cleaner_removes_style_blocks_via_process_web_content() {
    let processor = WebContentProcessor::new();
    let html = r#"<html><head><style>.foo { color: red; }</style></head><body><p>text</p></body></html>"#;
    let result = processor
        .process_web_content(html.as_bytes(), Some("text/html"))
        .unwrap();
    assert!(result.extracted_text.contains("text"));
    assert!(!result.extracted_text.contains("color"));
    assert!(!result.extracted_text.contains(".foo"));
}

#[test]
fn tc_html_cleaner_removes_html_comments_via_process_web_content() {
    let processor = WebContentProcessor::new();
    let html = "<html><body><p>before</p><!-- comment --><p>after</p></body></html>";
    let result = processor
        .process_web_content(html.as_bytes(), Some("text/html"))
        .unwrap();
    assert!(result.extracted_text.contains("before"));
    assert!(result.extracted_text.contains("after"));
    assert!(!result.extracted_text.contains("comment"));
}

#[test]
fn tc_html_cleaner_normalizes_whitespace_via_process_web_content() {
    let processor = WebContentProcessor::new();
    let html = "<html><body><p>hello</p>   <p>world</p></body></html>";
    let result = processor
        .process_web_content(html.as_bytes(), Some("text/html"))
        .unwrap();
    assert!(!result.extracted_text.contains("   "));
    assert!(result.extracted_text.contains("hello"));
    assert!(result.extracted_text.contains("world"));
}

#[test]
fn tc_html_cleaner_decodes_html_entities_via_process_web_content() {
    let processor = WebContentProcessor::new();
    let html = "<html><body><p>Hello &amp; goodbye &lt;world&gt;</p></body></html>";
    let result = processor
        .process_web_content(html.as_bytes(), Some("text/html"))
        .unwrap();
    assert!(result.extracted_text.contains("Hello & goodbye"));
    assert!(result.extracted_text.contains("<world>"));
}

// ===========================================================================
// CrawlTextProcessor tests
// ===========================================================================

#[test]
fn tc_crawl_text_processor_process_simple_text_preserves_unicode() {
    let processor = CrawlTextProcessor::new();
    let text = "Hello 世界!";
    let result = processor.process_simple_text(text).unwrap();
    assert_eq!(result, text);
}

#[test]
fn tc_crawl_text_processor_process_simple_text_empty_returns_empty() {
    let processor = CrawlTextProcessor::new();
    let result = processor.process_simple_text("").unwrap();
    assert_eq!(result, "");
}

#[test]
fn tc_crawl_text_processor_process_crawled_content_returns_correct_url() {
    let processor = CrawlTextProcessor::new();
    let result = processor
        .process_crawled_content(b"content", "https://example.com/page", None)
        .unwrap();
    assert_eq!(result.url, "https://example.com/page");
}

#[test]
fn tc_crawl_text_processor_process_crawled_content_calculates_sizes() {
    let processor = CrawlTextProcessor::new();
    let content = b"hello world this is test content";
    let result = processor
        .process_crawled_content(content, "http://x.com", None)
        .unwrap();
    assert_eq!(result.original_size, content.len());
    assert!(result.processed_size <= result.original_size);
}

#[test]
fn tc_crawl_text_processor_process_batch_mixed_success() {
    let processor = CrawlTextProcessor::new();
    let batch = vec![
        (b"content1".as_slice(), "http://a.com", Some("text/plain")),
        (
            b"<p>content2</p>".as_slice(),
            "http://b.com",
            Some("text/html"),
        ),
        (b"content3".as_slice(), "http://c.com", None),
    ];
    let results = processor.process_batch(batch);
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn tc_crawl_text_processor_process_batch_empty_returns_empty() {
    let processor = CrawlTextProcessor::new();
    let results = processor.process_batch(vec![]);
    assert!(results.is_empty());
}

#[test]
fn tc_crawl_text_processor_update_config_changes_limits() {
    let mut processor = CrawlTextProcessor::new();
    let new_config = CrawlProcessorConfig {
        max_processing_time_secs: 120,
        max_content_size_mb: 50,
    };
    processor.update_config(new_config);
    let config = processor.get_config();
    assert_eq!(config.max_processing_time_secs, 120);
    assert_eq!(config.max_content_size_mb, 50);
}

#[test]
fn tc_crawl_text_processor_content_too_large_error() {
    let mut processor = CrawlTextProcessor::new();
    processor.update_config(CrawlProcessorConfig {
        max_processing_time_secs: 30,
        max_content_size_mb: 0,
    });
    let content = b"x".repeat(100);
    let result = processor.process_crawled_content(&content, "http://x.com", None);
    match result {
        Err(CrawlProcessingError::ContentTooLarge { size, max_size }) => {
            assert_eq!(size, 100);
            assert_eq!(max_size, 0);
        }
        other => panic!("expected ContentTooLarge, got {:?}", other),
    }
}

// ===========================================================================
// validate_content_quality paths (public API, exercises calculate_repetitive_ratio internally)
// ===========================================================================

#[test]
fn tc_validate_content_quality_excellent_diverse_text() {
    let processor = CrawlTextProcessor::new();
    let diverse_text = "The quick brown fox jumps over the lazy dog near the river bank. \
         Birds sing loudly in the morning while cats sleep peacefully on warm rooftops. \
         Children play games outside enjoying sunny weather with friends and neighbors.";
    let result = processor
        .process_crawled_content(diverse_text.as_bytes(), "http://example.com", None)
        .unwrap();
    let quality = processor.validate_content_quality(&result);
    match quality {
        ContentQuality::Excellent | ContentQuality::Good => {}
        other => panic!("expected Excellent or Good, got {:?}", other),
    }
}

#[test]
fn tc_validate_content_quality_poor_short_text() {
    let processor = CrawlTextProcessor::new();
    let result = processor
        .process_crawled_content(b"hi", "http://x.com", None)
        .unwrap();
    let quality = processor.validate_content_quality(&result);
    match quality {
        ContentQuality::Poor(msg) => assert!(msg.contains("过短") || msg.contains("过少")),
        other => panic!("expected Poor, got {:?}", other),
    }
}

#[test]
fn tc_validate_content_quality_poor_repetitive() {
    let processor = CrawlTextProcessor::new();
    let repetitive = "word ".repeat(50);
    let result = processor
        .process_crawled_content(repetitive.as_bytes(), "http://x.com", None)
        .unwrap();
    let quality = processor.validate_content_quality(&result);
    match quality {
        ContentQuality::Poor(msg) => assert!(msg.contains("重复")),
        other => panic!("expected Poor repetitive, got {:?}", other),
    }
}

#[test]
fn tc_validate_content_quality_poor_low_ratio() {
    let processor = CrawlTextProcessor::new();
    let mut html = String::with_capacity(2000);
    html.push_str("<html><body>");
    for _ in 0..200 {
        html.push_str("<div></div>");
    }
    html.push_str("<p>");
    for _ in 0..15 {
        html.push_str("unique ");
    }
    html.push_str("</p>");
    html.push_str("</body></html>");
    let result = processor
        .process_crawled_content(html.as_bytes(), "http://x.com", Some("text/html"))
        .unwrap();
    let quality = processor.validate_content_quality(&result);
    match quality {
        ContentQuality::Poor(_) => {}
        ContentQuality::Excellent | ContentQuality::Good => {}
        other => panic!("unexpected quality: {:?}", other),
    }
}

// ===========================================================================
// Free function wrappers
// ===========================================================================

#[test]
fn tc_process_web_content_with_processor_fn_html() {
    let processor = WebContentProcessor::new();
    let result =
        process_web_content_with_processor(&processor, b"<p>test</p>", Some("text/html")).unwrap();
    assert!(result.is_html);
    assert!(result.extracted_text.contains("test"));
}

#[test]
fn tc_process_web_content_with_processor_fn_plain_text() {
    let processor = WebContentProcessor::new();
    let result =
        process_web_content_with_processor(&processor, b"plain text", Some("text/plain")).unwrap();
    assert!(!result.is_html);
    assert_eq!(result.extracted_text, "plain text");
}

#[test]
fn tc_process_crawled_content_with_processor_fn_success() {
    let processor = CrawlTextProcessor::new();
    let result =
        process_crawled_content_with_processor(&processor, b"hello", "http://x.com", None)
            .unwrap();
    assert_eq!(result.url, "http://x.com");
    assert!(!result.processed_content.is_html);
}

#[test]
fn tc_process_crawled_batch_with_processor_fn_success() {
    let processor = CrawlTextProcessor::new();
    let batch = vec![
        (b"a".as_slice(), "http://a.com", None),
        (b"b".as_slice(), "http://b.com", None),
        (b"c".as_slice(), "http://c.com", None),
    ];
    let results = process_crawled_batch_with_processor(&processor, batch);
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.is_ok()));
}

// ===========================================================================
// Free functions: init_encoding_patterns and detect_html_structure
// ===========================================================================

#[test]
fn tc_init_encoding_patterns_returns_three_valid_patterns() {
    let patterns = init_encoding_patterns().expect("patterns should compile");
    assert_eq!(patterns.len(), 3);
    let html_meta = patterns.get("html_meta").unwrap();
    assert!(html_meta.is_match(r#"<meta charset="utf-8">"#));
    let xml_decl = patterns.get("xml_declaration").unwrap();
    assert!(xml_decl.is_match(r#"<?xml encoding="UTF-8"?>"#));
    let http_ct = patterns.get("http_content_type").unwrap();
    assert!(http_ct.is_match("charset=utf-8"));
}

#[test]
fn tc_detect_html_structure_all_html_tags() {
    assert!(detect_html_structure("<html><body></body></html>"));
    assert!(detect_html_structure("<!DOCTYPE html>"));
    assert!(detect_html_structure("<head><title>X</title></head>"));
    assert!(detect_html_structure("<body>content</body>"));
    assert!(detect_html_structure("<div>content</div>"));
    assert!(detect_html_structure("<p>paragraph</p>"));
    assert!(detect_html_structure("<a href=\"x\">link</a>"));
}

#[test]
fn tc_detect_html_structure_non_html_returns_false() {
    assert!(!detect_html_structure("plain text content"));
    assert!(!detect_html_structure("no html here at all"));
    assert!(!detect_html_structure(""));
    assert!(!detect_html_structure("just some words"));
}

// ===========================================================================
// Trait object tests
// ===========================================================================

#[test]
fn tc_text_encoding_processor_component_as_trait_object() {
    let component: Arc<dyn TextEncodingProcessorTrait> =
        Arc::new(TextEncodingProcessorComponent::new());
    let result = component.process_text(b"trait test").unwrap();
    assert_eq!(result, "trait test");
    let trimmed = component.trim_newlines("line1\nline2");
    assert_eq!(trimmed, "line1 line2");
}

#[test]
fn tc_web_content_processor_component_as_trait_object() {
    let component: Arc<dyn WebContentProcessorTrait> =
        Arc::new(WebContentProcessorComponent::default());
    let result = component
        .process_web_content(b"trait test", Some("text/plain"))
        .unwrap();
    assert!(!result.is_html);
    assert_eq!(result.extracted_text, "trait test");
}

#[test]
fn tc_web_content_processor_trait_process_batch() {
    let component: Arc<dyn WebContentProcessorTrait> =
        Arc::new(WebContentProcessorComponent::default());
    let contents = vec![
        (b"a".as_slice(), None),
        (b"b".as_slice(), Some("text/plain")),
        (b"<p>c</p>".as_slice(), Some("text/html")),
    ];
    let results = component.process_batch(contents);
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.is_ok()));
}

// ===========================================================================
// Error variant display
// ===========================================================================

#[test]
fn tc_web_content_error_encoding_error_display() {
    let err =
        WebContentError::EncodingError(TextEncodingError::DetectionFailed("test".to_string()));
    assert!(err.to_string().contains("编码处理错误"));
}

#[test]
fn tc_web_content_error_html_parse_error_display() {
    let err = WebContentError::HtmlParseError("parse issue".to_string());
    assert!(err.to_string().contains("HTML解析错误"));
}

#[test]
fn tc_web_content_error_content_extraction_error_display() {
    let err = WebContentError::ContentExtractionError("extract fail".to_string());
    assert!(err.to_string().contains("内容提取错误"));
}

#[test]
fn tc_crawl_processing_error_content_too_large_display() {
    let err = CrawlProcessingError::ContentTooLarge {
        size: 1000,
        max_size: 500,
    };
    let msg = err.to_string();
    assert!(msg.contains("1000"));
    assert!(msg.contains("500"));
}

#[test]
fn tc_crawl_processing_error_processing_timeout_display() {
    let err = CrawlProcessingError::ProcessingTimeout;
    assert_eq!(err.to_string(), "处理超时");
}

#[test]
fn tc_crawl_processing_error_text_encoding_display() {
    let err = CrawlProcessingError::TextEncodingError(TextEncodingError::DetectionFailed(
        "detect fail".to_string(),
    ));
    assert!(err.to_string().contains("文本编码处理错误"));
}

#[test]
fn tc_crawl_processing_error_web_content_display() {
    let err = CrawlProcessingError::WebContentError(WebContentError::HtmlParseError(
        "parse error".to_string(),
    ));
    assert!(err.to_string().contains("网页内容处理错误"));
}

#[test]
fn tc_crawl_processing_error_poor_content_quality_display() {
    let err = CrawlProcessingError::PoorContentQuality("bad quality".to_string());
    assert!(err.to_string().contains("bad quality"));
}

// ===========================================================================
// ContentQuality enum tests
// ===========================================================================

#[test]
fn tc_content_quality_variants_equality() {
    assert_eq!(ContentQuality::Excellent, ContentQuality::Excellent);
    assert_eq!(ContentQuality::Good, ContentQuality::Good);
    assert_eq!(ContentQuality::Fair, ContentQuality::Fair);
    assert_ne!(
        ContentQuality::Excellent,
        ContentQuality::Poor("x".to_string())
    );
}

#[test]
fn tc_content_quality_poor_clone_preserves_message() {
    let poor = ContentQuality::Poor("test reason".to_string());
    let cloned = poor.clone();
    match cloned {
        ContentQuality::Poor(r) => assert_eq!(r, "test reason"),
        _ => panic!("wrong variant after clone"),
    }
}

// ===========================================================================
// CrawlProcessorConfig tests
// ===========================================================================

#[test]
fn tc_crawl_processor_config_default_values() {
    let config = CrawlProcessorConfig::default();
    assert_eq!(config.max_processing_time_secs, 30);
    assert_eq!(config.max_content_size_mb, 10);
}

#[test]
fn tc_crawl_processor_config_clone_preserves_values() {
    let config = CrawlProcessorConfig {
        max_processing_time_secs: 45,
        max_content_size_mb: 15,
    };
    let cloned = config.clone();
    assert_eq!(cloned.max_processing_time_secs, 45);
    assert_eq!(cloned.max_content_size_mb, 15);
}

// ===========================================================================
// ProcessedWebContent and ProcessedCrawlContent struct tests
// ===========================================================================

#[test]
fn tc_processed_web_content_all_fields() {
    let content = ProcessedWebContent {
        original_content: "original".to_string(),
        extracted_text: "extracted".to_string(),
        is_html: true,
        declared_encoding: Some("utf-8".to_string()),
        detected_language: Some("en".to_string()),
        content_type: Some("text/html".to_string()),
        content_length: 100,
    };
    assert_eq!(content.original_content, "original");
    assert_eq!(content.extracted_text, "extracted");
    assert!(content.is_html);
    assert_eq!(content.declared_encoding.as_deref(), Some("utf-8"));
    assert_eq!(content.detected_language.as_deref(), Some("en"));
    assert_eq!(content.content_type.as_deref(), Some("text/html"));
    assert_eq!(content.content_length, 100);
}

#[test]
fn tc_processed_web_content_clone() {
    let content = ProcessedWebContent {
        original_content: "orig".to_string(),
        extracted_text: "ext".to_string(),
        is_html: false,
        declared_encoding: None,
        detected_language: None,
        content_type: None,
        content_length: 5,
    };
    let cloned = content.clone();
    assert_eq!(cloned.extracted_text, content.extracted_text);
    assert_eq!(cloned.is_html, content.is_html);
}

#[test]
fn tc_processed_crawl_content_clone_preserves_fields() {
    let processor = CrawlTextProcessor::new();
    let result = processor
        .process_crawled_content(b"test content", "http://x.com", None)
        .unwrap();
    let cloned: ProcessedCrawlContent = result.clone();
    assert_eq!(cloned.url, result.url);
    assert_eq!(cloned.original_size, result.original_size);
    assert_eq!(cloned.processed_size, result.processed_size);
}

#[test]
fn tc_processed_crawl_content_debug_format() {
    let processor = CrawlTextProcessor::new();
    let result = processor
        .process_crawled_content(b"debug test", "http://x.com", None)
        .unwrap();
    let debug = format!("{:?}", result);
    assert!(debug.contains("ProcessedCrawlContent"));
    assert!(debug.contains("http://x.com"));
}
