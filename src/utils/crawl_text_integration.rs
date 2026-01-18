// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::utils::text_processing::processor::{CrawlProcessingError, CrawlTextProcessor};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

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
            .zip(responses.into_iter())
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
}
