// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::utils::crawl_text_processor::{CrawlProcessingError, CrawlTextProcessor};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// 爬虫文本处理集成器
///
/// 将文本编码处理功能集成到现有的爬虫系统中
/// 提供无缝的文本处理体验，无需修改现有代码结构
pub struct CrawlTextIntegration {
    processor: Arc<RwLock<CrawlTextProcessor>>,
    enabled: bool,
}

impl CrawlTextIntegration {
    /// 创建新的文本处理集成器
    pub fn new(enabled: bool) -> Self {
        Self {
            processor: Arc::new(RwLock::new(CrawlTextProcessor::new())),
            enabled,
        }
    }

    /// 启用文本处理
    pub async fn enable(&self) {
        let _processor = self.processor.write().await;
        info!("启用爬虫文本处理功能");
        // 可以在这里添加初始化逻辑
    }

    /// 禁用文本处理
    pub async fn disable(&self) {
        let _processor = self.processor.write().await;
        info!("禁用爬虫文本处理功能");
        // 可以在这里添加清理逻辑
    }

    /// 处理爬虫响应内容
    ///
    /// 这是主要的集成点，在现有爬虫流程中调用
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
                original_content: String::from_utf8_lossy(content).to_string(),
                processed_content: String::from_utf8_lossy(content).to_string(),
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

                // 即使处理失败，也返回原始内容，避免中断爬虫流程
                Ok(ProcessedScrapeResponse {
                    original_content: String::from_utf8_lossy(content).to_string(),
                    processed_content: String::from_utf8_lossy(content).to_string(),
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

    /// 批量处理多个爬虫响应
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
                        original_content: String::from_utf8_lossy(&response.content).to_string(),
                        processed_content: String::from_utf8_lossy(&response.content).to_string(),
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
                        original_content: String::from_utf8_lossy(&response.content).to_string(),
                        processed_content: String::from_utf8_lossy(&response.content).to_string(),
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

    /// 更新处理器配置
    pub async fn update_config(&self, config: CrawlTextProcessorConfig) {
        let mut processor = self.processor.write().await;
        processor.update_config(config.clone());
        info!("更新爬虫文本处理器配置: {:?}", config.clone());
    }

    /// 获取处理器状态
    pub async fn get_status(&self) -> CrawlTextIntegrationStatus {
        let processor = self.processor.read().await;
        let config = processor.get_config();

        CrawlTextIntegrationStatus {
            enabled: self.enabled,
            config,
            uptime: std::time::Duration::from_secs(0), // 可以添加实际的运行时间
            processed_count: 0,                        // 可以添加处理计数
            error_count: 0,                            // 可以添加错误计数
        }
    }

    /// 验证内容质量
    pub async fn validate_content(&self, content: &ProcessedScrapeResponse) -> ContentQuality {
        if !self.enabled {
            return ContentQuality::Good; // 如果禁用，默认返回Good
        }

        // 这里可以添加更复杂的内容质量验证逻辑
        if content.processed_content.len() < 50 {
            ContentQuality::Poor("内容过短".to_string())
        } else if content.processing_success {
            ContentQuality::Excellent
        } else {
            ContentQuality::Fair
        }
    }
}

/// 爬虫响应输入
#[derive(Debug, Clone)]
pub struct ScrapeResponseInput {
    pub content: Vec<u8>,
    pub url: String,
    pub content_type: Option<String>,
    pub status_code: u16,
}

/// 处理后的爬虫响应
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

/// 爬虫文本处理器配置
pub use crate::utils::crawl_text_processor::CrawlProcessorConfig as CrawlTextProcessorConfig;

/// 集成器状态
#[derive(Debug, Clone)]
pub struct CrawlTextIntegrationStatus {
    pub enabled: bool,
    pub config: CrawlTextProcessorConfig,
    pub uptime: std::time::Duration,
    pub processed_count: u64,
    pub error_count: u64,
}

/// 内容质量评级
#[derive(Debug, Clone, PartialEq)]
pub enum ContentQuality {
    Excellent,
    Good,
    Fair,
    Poor(String),
}

/// 便捷函数：创建集成器
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

        assert_eq!(result.processing_success, false);
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

        assert_eq!(result.processing_success, true);
        assert!(result.processed_content.contains("测试页面"));
        assert!(result.processed_content.contains("中文和English内容"));
        assert_eq!(result.is_html, true);
    }

    #[tokio::test]
    async fn test_batch_processing() {
        let integration = CrawlTextIntegration::new(true);

        let responses = vec![
            ScrapeResponseInput {
                content: "<html><body><h1>页面1</h1></body></html>"
                    .as_bytes()
                    .to_vec(),
                url: "http://example1.com".to_string(),
                content_type: Some("text/html".to_string()),
                status_code: 200,
            },
            ScrapeResponseInput {
                content: b"Plain text content".to_vec(),
                url: "http://example2.com".to_string(),
                content_type: Some("text/plain".to_string()),
                status_code: 200,
            },
        ];

        let results = integration.process_batch_scrape_responses(responses).await;
        assert_eq!(results.len(), 2);

        // 第一个结果应该是HTML处理
        let first_result = results[0].as_ref().unwrap();
        assert_eq!(first_result.is_html, true);
        assert!(first_result.processed_content.contains("页面1"));

        // 第二个结果应该是纯文本
        let second_result = results[1].as_ref().unwrap();
        assert_eq!(second_result.is_html, false);
        assert_eq!(second_result.processed_content, "Plain text content");
    }
}
