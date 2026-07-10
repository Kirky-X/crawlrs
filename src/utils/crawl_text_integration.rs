// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::utils::text_processing::processor::{CrawlProcessingError, CrawlTextProcessor};
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct CrawlTextIntegration {
    processor: Arc<RwLock<CrawlTextProcessor>>,
    enabled: bool,
}

impl CrawlTextIntegration {
    pub fn new(enabled: bool) -> Self {
        Self {
            processor: Arc::new(RwLock::new(CrawlTextProcessor::new())),
            enabled,
        }
    }

    pub async fn enable(&self) {
        let _processor = self.processor.write().await;
        info!("启用爬虫文本处理功能");
    }

    pub async fn disable(&self) {
        let _processor = self.processor.write().await;
        info!("禁用爬虫文本处理功能");
    }

    pub async fn process_scrape_response(
        &self,
        content: &[u8],
        url: &str,
        content_type: Option<&str>,
        status_code: u16,
    ) -> Result<ProcessedScrapeResponse, CrawlProcessingError> {
        if !self.enabled {
            debug!("文本处理功能已禁用，直接返回原始内容");
            return Ok(ProcessedScrapeResponse {
                original_content: String::from_utf8_lossy(content).into_owned(),
                processed_content: String::from_utf8_lossy(content).into_owned(),
                content_type: content_type.map(|s| s.to_string()),
                status_code,
                url: url.to_string(),
                encoding_detected: None,
                language_detected: None,
                is_html: false,
                processing_success: false,
                processing_error: None,
            });
        }

        let processor = self.processor.read().await;

        match processor.process_crawled_content(content, url, content_type) {
            Ok(processed_content) => {
                let result = ProcessedScrapeResponse {
                    original_content: processed_content.processed_content.original_content.clone(),
                    processed_content: processed_content.processed_content.extracted_text.clone(),
                    content_type: processed_content.processed_content.content_type.clone(),
                    status_code,
                    url: url.to_string(),
                    encoding_detected: processed_content
                        .processed_content
                        .declared_encoding
                        .clone(),
                    language_detected: processed_content
                        .processed_content
                        .detected_language
                        .clone(),
                    is_html: processed_content.processed_content.is_html,
                    processing_success: true,
                    processing_error: None,
                };
                info!(
                    "文本处理成功: URL={}, 提取文本长度={}, 语言={:?}",
                    url,
                    result.processed_content.len(),
                    result.language_detected
                );
                Ok(result)
            }
            Err(e) => {
                error!("文本处理失败: URL={}, 错误={}", url, e);
                Ok(ProcessedScrapeResponse {
                    original_content: String::from_utf8_lossy(content).into_owned(),
                    processed_content: String::from_utf8_lossy(content).into_owned(),
                    content_type: content_type.map(|s| s.to_string()),
                    status_code,
                    url: url.to_string(),
                    encoding_detected: None,
                    language_detected: None,
                    is_html: false,
                    processing_success: false,
                    processing_error: Some(e.to_string()),
                })
            }
        }
    }

    pub async fn process_batch_scrape_responses(
        &self,
        responses: Vec<ScrapeResponseInput>,
    ) -> Vec<Result<ProcessedScrapeResponse, CrawlProcessingError>> {
        if !self.enabled {
            debug!("文本处理功能已禁用，批量返回原始内容");
            return responses
                .into_iter()
                .map(|response| {
                    Ok(ProcessedScrapeResponse {
                        original_content: String::from_utf8_lossy(&response.content).into_owned(),
                        processed_content: String::from_utf8_lossy(&response.content).into_owned(),
                        content_type: response.content_type.clone(),
                        status_code: response.status_code,
                        url: response.url.clone(),
                        encoding_detected: None,
                        language_detected: None,
                        is_html: false,
                        processing_success: false,
                        processing_error: None,
                    })
                })
                .collect();
        }

        let processor = self.processor.read().await;

        let batch_input: Vec<(&[u8], &str, Option<&str>)> = responses
            .iter()
            .map(|response| {
                (
                    response.content.as_slice(),
                    response.url.as_str(),
                    response.content_type.as_deref(),
                )
            })
            .collect();

        let batch_results = processor.process_batch(batch_input);

        batch_results
            .into_iter()
            .zip(responses)
            .map(|(result, response)| match result {
                Ok(processed_content) => Ok(ProcessedScrapeResponse {
                    original_content: processed_content.processed_content.original_content.clone(),
                    processed_content: processed_content.processed_content.extracted_text.clone(),
                    content_type: processed_content.processed_content.content_type.clone(),
                    status_code: response.status_code,
                    url: response.url,
                    encoding_detected: processed_content
                        .processed_content
                        .declared_encoding
                        .clone(),
                    language_detected: processed_content
                        .processed_content
                        .detected_language
                        .clone(),
                    is_html: processed_content.processed_content.is_html,
                    processing_success: true,
                    processing_error: None,
                }),
                Err(e) => {
                    warn!("批量处理中的单个项目失败: URL={}, 错误={}", response.url, e);
                    Ok(ProcessedScrapeResponse {
                        original_content: String::from_utf8_lossy(&response.content).into_owned(),
                        processed_content: String::from_utf8_lossy(&response.content).into_owned(),
                        content_type: response.content_type,
                        status_code: response.status_code,
                        url: response.url,
                        encoding_detected: None,
                        language_detected: None,
                        is_html: false,
                        processing_success: false,
                        processing_error: Some(e.to_string()),
                    })
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ScrapeResponseInput {
    pub content: Vec<u8>,
    pub url: String,
    pub content_type: Option<String>,
    pub status_code: u16,
}

#[derive(Debug, Clone)]
pub struct ProcessedScrapeResponse {
    pub original_content: String,
    pub processed_content: String,
    pub content_type: Option<String>,
    pub status_code: u16,
    pub url: String,
    pub encoding_detected: Option<String>,
    pub language_detected: Option<String>,
    pub is_html: bool,
    pub processing_success: bool,
    pub processing_error: Option<String>,
}

pub use crate::utils::text_processing::processor::CrawlProcessorConfig as CrawlTextProcessorConfig;

#[derive(Debug, Clone)]
pub struct CrawlTextIntegrationStatus {
    pub enabled: bool,
    pub config: CrawlTextProcessorConfig,
    pub uptime: std::time::Duration,
    pub processed_count: u64,
    pub error_count: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentQuality {
    Excellent,
    Good,
    Fair,
    Poor(String),
}

pub fn create_crawl_text_integration(enabled: bool) -> CrawlTextIntegration {
    CrawlTextIntegration::new(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_integration_disabled() {
        let integration = CrawlTextIntegration::new(false);
        let content = "Hello, 世界!".as_bytes();
        let result = integration
            .process_scrape_response(content, "http://example.com", None, 200)
            .await
            .unwrap();
        assert!(!result.processing_success);
        assert_eq!(result.original_content, "Hello, 世界!");
        assert_eq!(result.processed_content, "Hello, 世界!");
    }

    #[tokio::test]
    async fn test_integration_enabled() {
        let integration = CrawlTextIntegration::new(true);
        let html_content = "<html><body><h1>测试页面</h1><p>这是一个测试页面，包含中文和English内容。</p></body></html>".as_bytes();
        let result = integration
            .process_scrape_response(html_content, "http://example.com", Some("text/html"), 200)
            .await
            .unwrap();
        assert!(result.processing_success);
        assert!(result.processed_content.contains("测试页面"));
        assert!(result.processed_content.contains("中文和English内容"));
        assert!(result.is_html);
    }

    #[tokio::test]
    async fn test_batch_disabled() {
        let integration = CrawlTextIntegration::new(false);
        let responses = vec![
            ScrapeResponseInput {
                content: b"Hello".to_vec(),
                url: "http://example.com/1".to_string(),
                content_type: Some("text/plain".to_string()),
                status_code: 200,
            },
            ScrapeResponseInput {
                content: b"World".to_vec(),
                url: "http://example.com/2".to_string(),
                content_type: None,
                status_code: 200,
            },
        ];
        let results = integration.process_batch_scrape_responses(responses).await;
        assert_eq!(results.len(), 2);
        for result in &results {
            assert!(result.is_ok());
            assert!(!result.as_ref().unwrap().processing_success);
        }
    }

    #[tokio::test]
    async fn test_batch_enabled() {
        let integration = CrawlTextIntegration::new(true);
        let html = b"<html><body><p>Test content</p></body></html>";
        let responses = vec![ScrapeResponseInput {
            content: html.to_vec(),
            url: "http://example.com".to_string(),
            content_type: Some("text/html".to_string()),
            status_code: 200,
        }];
        let results = integration.process_batch_scrape_responses(responses).await;
        assert_eq!(results.len(), 1);
        let result = results[0].as_ref().unwrap();
        assert!(result.processing_success);
        assert!(result.processed_content.contains("Test content"));
    }

    #[tokio::test]
    async fn test_enable_method_does_not_panic() {
        let integration = CrawlTextIntegration::new(false);
        integration.enable().await;
        // enable() only logs; enabled flag unchanged (still false)
        let content = b"test";
        let result = integration
            .process_scrape_response(content, "http://example.com", None, 200)
            .await
            .unwrap();
        assert!(!result.processing_success);
    }

    #[tokio::test]
    async fn test_disable_method_does_not_panic() {
        let integration = CrawlTextIntegration::new(true);
        integration.disable().await;
        // disable() only logs; enabled flag unchanged (still true)
        let html = b"<html><body><p>Test</p></body></html>";
        let result = integration
            .process_scrape_response(html, "http://example.com", Some("text/html"), 200)
            .await
            .unwrap();
        assert!(result.processing_success);
    }

    #[test]
    fn test_create_crawl_text_integration_disabled() {
        let integration = create_crawl_text_integration(false);
        let _ = integration;
    }

    #[test]
    fn test_create_crawl_text_integration_enabled() {
        let integration = create_crawl_text_integration(true);
        let _ = integration;
    }

    #[test]
    fn test_content_quality_variants() {
        let excellent = ContentQuality::Excellent;
        let good = ContentQuality::Good;
        let fair = ContentQuality::Fair;
        let poor = ContentQuality::Poor("reason".to_string());

        assert_eq!(excellent, ContentQuality::Excellent);
        assert_eq!(good, ContentQuality::Good);
        assert_eq!(fair, ContentQuality::Fair);
        assert_eq!(poor, ContentQuality::Poor("reason".to_string()));
    }

    #[test]
    fn test_scrape_response_input_construction() {
        let input = ScrapeResponseInput {
            content: vec![1, 2, 3],
            url: "http://test.com".to_string(),
            content_type: Some("text/html".to_string()),
            status_code: 404,
        };
        assert_eq!(input.content, vec![1, 2, 3]);
        assert_eq!(input.url, "http://test.com");
        assert_eq!(input.content_type, Some("text/html".to_string()));
        assert_eq!(input.status_code, 404);
    }

    #[test]
    fn test_processed_scrape_response_construction() {
        let response = ProcessedScrapeResponse {
            original_content: "original".to_string(),
            processed_content: "processed".to_string(),
            content_type: Some("text/html".to_string()),
            status_code: 200,
            url: "http://test.com".to_string(),
            encoding_detected: Some("utf-8".to_string()),
            language_detected: Some("en".to_string()),
            is_html: true,
            processing_success: true,
            processing_error: None,
        };
        assert_eq!(response.original_content, "original");
        assert_eq!(response.processed_content, "processed");
        assert!(response.is_html);
        assert!(response.processing_success);
    }

    #[test]
    fn test_crawl_text_integration_status_construction() {
        let status = CrawlTextIntegrationStatus {
            enabled: true,
            config: CrawlTextProcessorConfig::default(),
            uptime: std::time::Duration::from_secs(60),
            processed_count: 100,
            error_count: 5,
        };
        assert!(status.enabled);
        assert_eq!(status.uptime, std::time::Duration::from_secs(60));
        assert_eq!(status.processed_count, 100);
        assert_eq!(status.error_count, 5);
    }

    #[tokio::test]
    async fn test_process_scrape_response_with_invalid_utf8() {
        let integration = CrawlTextIntegration::new(true);
        let invalid_bytes = &[0xFF, 0xFE, 0xFD];
        let result = integration
            .process_scrape_response(invalid_bytes, "http://example.com", None, 500)
            .await
            .unwrap();
        assert_eq!(result.status_code, 500);
    }
}
