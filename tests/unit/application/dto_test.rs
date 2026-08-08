// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! DTO tests for extraction fields (unify-extraction-api)
//!
//! Verifies that ScrapeRequestDto, CrawlConfigDto, and ExtractRequestDto
//! correctly accept the new extraction_prompt/extraction_schema fields
//! and serde aliases.

use crawlrs::application::dto::crawl_request::CrawlConfigDto;
use crawlrs::application::dto::scrape_request::ScrapeRequestDto;
use serde_json::json;

// =============================================================================
// ScrapeRequestDto — extraction_prompt / extraction_schema
// =============================================================================

#[test]
fn test_scrape_dto_extraction_prompt() {
    let json = json!({
        "url": "https://example.com",
        "extraction_prompt": "Extract the main title and all links"
    });
    let dto: ScrapeRequestDto = serde_json::from_value(json).expect("must parse");
    assert_eq!(
        dto.extraction_prompt,
        Some("Extract the main title and all links".to_string())
    );
    assert!(dto.extraction_rules.is_none());
    assert!(dto.extraction_schema.is_none());
}

#[test]
fn test_scrape_dto_extraction_schema() {
    let json = json!({
        "url": "https://example.com",
        "extraction_schema": {
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "links": { "type": "array", "items": { "type": "string" } }
            }
        }
    });
    let dto: ScrapeRequestDto = serde_json::from_value(json).expect("must parse");
    assert!(dto.extraction_prompt.is_none());
    assert!(dto.extraction_schema.is_some());
    let schema = dto.extraction_schema.unwrap();
    assert_eq!(schema["type"], "object");
}

#[test]
fn test_scrape_dto_all_three_extraction_fields() {
    let json = json!({
        "url": "https://example.com",
        "extraction_rules": {
            "title": { "selector": "h1", "attr": null, "is_array": false }
        },
        "extraction_prompt": "Extract all links",
        "extraction_schema": { "type": "object" }
    });
    let dto: ScrapeRequestDto = serde_json::from_value(json).expect("must parse");
    assert!(dto.extraction_rules.is_some());
    assert_eq!(dto.extraction_rules.unwrap().len(), 1);
    assert!(dto.extraction_prompt.is_some());
    assert!(dto.extraction_schema.is_some());
}

#[test]
fn test_scrape_dto_no_extraction_fields() {
    let json = json!({ "url": "https://example.com" });
    let dto: ScrapeRequestDto = serde_json::from_value(json).expect("must parse");
    assert!(dto.extraction_rules.is_none());
    assert!(dto.extraction_prompt.is_none());
    assert!(dto.extraction_schema.is_none());
}

// =============================================================================
// CrawlConfigDto — extraction_prompt / extraction_schema
// =============================================================================

#[test]
fn test_crawl_config_dto_extraction_prompt() {
    let json = json!({
        "max_depth": 2,
        "extraction_prompt": "Extract product names and prices"
    });
    let dto: CrawlConfigDto = serde_json::from_value(json).expect("must parse");
    assert_eq!(
        dto.extraction_prompt,
        Some("Extract product names and prices".to_string())
    );
    assert!(dto.extraction_rules.is_none());
    assert!(dto.extraction_schema.is_none());
}

#[test]
fn test_crawl_config_dto_extraction_schema() {
    let json = json!({
        "max_depth": 3,
        "extraction_schema": {
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "price": { "type": "number" }
            }
        }
    });
    let dto: CrawlConfigDto = serde_json::from_value(json).expect("must parse");
    assert!(dto.extraction_prompt.is_none());
    assert!(dto.extraction_schema.is_some());
}

#[test]
fn test_crawl_config_dto_all_three_extraction_fields() {
    let json = json!({
        "max_depth": 1,
        "extraction_rules": {
            "title": { "selector": "h1", "attr": null, "is_array": false }
        },
        "extraction_prompt": "Extract links",
        "extraction_schema": { "type": "object" }
    });
    let dto: CrawlConfigDto = serde_json::from_value(json).expect("must parse");
    assert!(dto.extraction_rules.is_some());
    assert!(dto.extraction_prompt.is_some());
    assert!(dto.extraction_schema.is_some());
}
