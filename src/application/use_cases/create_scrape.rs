// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! CreateScrapeUseCase - Use case for creating scrape tasks
//!
//! This module implements the use case for creating scrape tasks.
//! Uses the new EngineClient API for scraping operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use shaku::Interface;

use crate::application::dto::scrape_request::{
    ScrapeActionDto, ScrapeOptionsDto, ScrapeRequestDto,
};
use crate::domain::models::DomainError;
use crate::engines::engine_client::{
    EngineClient, HttpMethod, PageAction, ScrapeOptions, ScrapeRequest, ScrapeResponse,
    ScreenshotConfig, ScrollDirection,
};

// === Section: Use Case Definition ===

/// Trait for CreateScrapeUseCase - enables dependency injection
#[async_trait::async_trait]
pub trait CreateScrapeUseCaseTrait: Interface + Send + Sync {
    /// Execute the scrape use case with the given request DTO
    async fn execute(&self, request_dto: ScrapeRequestDto) -> Result<ScrapeResponse, DomainError>;
}

pub struct CreateScrapeUseCase {
    engine_client: Arc<EngineClient>,
}

// === Section: Implementation ===

impl CreateScrapeUseCase {
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self { engine_client }
    }

    /// Execute the scrape use case with the given request DTO.
    ///
    /// This method maps the DTO to a scrape request, executes it using
    /// the EngineClient, and returns the response or an error.
    ///
    /// # Arguments
    ///
    /// * `request_dto` - The scrape request DTO containing URL and options
    ///
    /// # Returns
    ///
    /// * `Ok(ScrapeResponse)` on success
    /// * `Err(DomainError)` on failure with specific error type
    pub async fn execute(
        &self,
        request_dto: ScrapeRequestDto,
    ) -> Result<ScrapeResponse, DomainError> {
        self._execute_impl(request_dto).await
    }

    async fn _execute_impl(
        &self,
        request_dto: ScrapeRequestDto,
    ) -> Result<ScrapeResponse, DomainError> {
        let scrape_request = self.map_dto_to_request(request_dto)?;
        self.engine_client
            .scrape(&scrape_request)
            .await
            .map_err(map_engine_error)
    }

    fn map_dto_to_request(&self, dto: ScrapeRequestDto) -> Result<ScrapeRequest, DomainError> {
        let options = dto.options.unwrap_or(ScrapeOptionsDto {
            headers: None,
            wait_for: None,
            timeout: None,
            js_rendering: None,
            screenshot: None,
            screenshot_options: None,
            mobile: None,
            proxy: None,
            skip_tls_verification: None,
            needs_tls_fingerprint: None,
            use_fire_engine: None,
        });

        let headers = self.parse_headers(options.headers)?;

        let screenshot_config = options.screenshot_options.map(|opts| ScreenshotConfig {
            full_page: opts.full_page.unwrap_or(false),
            selector: opts.selector,
            quality: opts.quality,
            format: opts.format,
        });

        let scrape_options = ScrapeOptions {
            method: HttpMethod::Get,
            needs_js: options.js_rendering.unwrap_or(false),
            needs_screenshot: options.screenshot.unwrap_or(false),
            mobile: options.mobile.unwrap_or(false),
            timeout: Duration::from_secs(options.timeout.unwrap_or(30)),
            body: None,
            sync_wait_ms: dto.sync_wait_ms.unwrap_or(0),
            actions: self.parse_actions(dto.actions),
            screenshot_config,
            proxy: options.proxy,
            skip_tls_verification: options.skip_tls_verification.unwrap_or(false),
            headers,
            needs_tls_fingerprint: options.needs_tls_fingerprint.unwrap_or(false),
            use_fire_engine: options.use_fire_engine.unwrap_or(false),
        };

        Ok(ScrapeRequest::new(dto.url).with_options(scrape_options))
    }

    fn parse_actions(&self, dto_actions: Option<Vec<ScrapeActionDto>>) -> Vec<PageAction> {
        dto_actions
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| match a {
                ScrapeActionDto::Wait { milliseconds } => Some(PageAction::Wait { milliseconds }),
                ScrapeActionDto::Click { selector } => Some(PageAction::Click { selector }),
                ScrapeActionDto::Scroll { direction } => {
                    let rust_direction = match direction.as_str() {
                        "up" => ScrollDirection::Up,
                        "down" => ScrollDirection::Down,
                        "top" => ScrollDirection::Top,
                        "bottom" => ScrollDirection::Bottom,
                        _ => ScrollDirection::Down,
                    };
                    Some(PageAction::Scroll {
                        direction: rust_direction,
                    })
                }
                ScrapeActionDto::Screenshot { .. } => {
                    // Screenshot is handled via ScrapeOptions.needs_screenshot
                    None
                }
                ScrapeActionDto::Input { selector, text } => {
                    Some(PageAction::Input { selector, text })
                }
            })
            .collect()
    }

    fn parse_headers(
        &self,
        headers_value: Option<Value>,
    ) -> Result<HashMap<String, String>, DomainError> {
        match headers_value {
            Some(Value::Object(map)) => map
                .into_iter()
                .map(|(k, v)| {
                    let v_str = v.as_str().ok_or_else(|| {
                        DomainError::ValidationError(format!("Invalid header value for key: {}", k))
                    })?;
                    Ok((k, v_str.to_string()))
                })
                .collect(),
            Some(_) => Err(DomainError::ValidationError(
                "Headers must be a map of string key-value pairs".to_string(),
            )),
            None => Ok(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl CreateScrapeUseCaseTrait for CreateScrapeUseCase {
    async fn execute(&self, request_dto: ScrapeRequestDto) -> Result<ScrapeResponse, DomainError> {
        self._execute_impl(request_dto).await
    }
}

// === Section: Module Organization ===

// Note: In a real application, you might have more complex logic here,
// such as handling different extraction rules, webhooks, etc.
// This implementation focuses on the core scraping request flow.

/// Map EngineError to DomainError with specific error types
fn map_engine_error(engine_error: crate::engines::engine_client::EngineError) -> DomainError {
    match engine_error {
        crate::engines::engine_client::EngineError::Timeout(duration) => {
            DomainError::TimeoutError(format!("Request timed out after {:?}", duration))
        }
        crate::engines::engine_client::EngineError::SsrfProtection(msg) => {
            DomainError::SecurityError(msg)
        }
        crate::engines::engine_client::EngineError::InvalidUrl(msg) => DomainError::InvalidUrl(msg),
        crate::engines::engine_client::EngineError::RequestFailed(msg)
        | crate::engines::engine_client::EngineError::BrowserError(msg) => {
            DomainError::NetworkError(msg)
        }
        crate::engines::engine_client::EngineError::NoEnginesAvailable => {
            DomainError::EngineError("No engines available".to_string())
        }
        crate::engines::engine_client::EngineError::Internal(msg) => DomainError::EngineError(msg),
        crate::engines::engine_client::EngineError::AllEnginesFailed(msg) => {
            DomainError::EngineError(msg)
        }
        crate::engines::engine_client::EngineError::Expired => {
            DomainError::EngineError("Engine request expired".to_string())
        }
        crate::engines::engine_client::EngineError::Other(msg) => DomainError::EngineError(msg),
    }
}
