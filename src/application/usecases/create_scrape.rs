// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::application::dto::scrape_request::{
    ScrapeOptionsDto, ScrapeRequestDto, ScreenshotOptionsDto,
};
use crate::domain::models::task::DomainError;
use crate::engines::router::EngineRouter;
use crate::engines::traits::{ScrapeRequest, ScrapeResponse, ScreenshotConfig};

// === Section: Use Case Definition ===

pub struct CreateScrapeUseCase {
    engine_router: Arc<EngineRouter>,
}

// === Section: Implementation ===

impl CreateScrapeUseCase {
    pub fn new(engine_router: Arc<EngineRouter>) -> Self {
        Self { engine_router }
    }

    pub async fn execute(
        &self,
        request_dto: ScrapeRequestDto,
    ) -> Result<ScrapeResponse, DomainError> {
        let scrape_request = self.map_dto_to_request(request_dto)?;
        self.engine_router
            .route(&scrape_request)
            .await
            .map_err(|e| DomainError::EngineError(e.to_string()))
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
        let screenshot_config = self.parse_screenshot_config(options.screenshot_options);

        Ok(ScrapeRequest {
            url: dto.url,
            headers,
            timeout: Duration::from_secs(options.timeout.unwrap_or(30)),
            needs_js: options.js_rendering.unwrap_or(false),
            needs_screenshot: options.screenshot.unwrap_or(false),
            screenshot_config,
            mobile: options.mobile.unwrap_or(false),
            proxy: options.proxy,
            skip_tls_verification: options.skip_tls_verification.unwrap_or(false),
            needs_tls_fingerprint: options.needs_tls_fingerprint.unwrap_or(false),
            use_fire_engine: options.use_fire_engine.unwrap_or(false),
        })
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

    fn parse_screenshot_config(
        &self,
        options: Option<ScreenshotOptionsDto>,
    ) -> Option<ScreenshotConfig> {
        options.map(|opts| ScreenshotConfig {
            full_page: opts.full_page.unwrap_or(false),
            selector: opts.selector,
            quality: opts.quality,
            format: opts.format,
        })
    }
}

// === Section: Module Organization ===

// Note: In a real application, you might have more complex logic here,
// such as handling different extraction rules, webhooks, etc.
// This implementation focuses on the core scraping request flow.
