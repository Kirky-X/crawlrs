// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::{http::StatusCode, response::IntoResponse, Extension, Json};
use serde_json::json;

use crate::application::dto::extract_request::{
    ExtractRequestDto, ExtractResponseDto, ExtractResultDto,
};
use crate::config::settings::Settings;
use crate::domain::services::extraction_service::{ExtractionRule, ExtractionService};
use crate::domain::services::llm_service::LLMService;
use crate::engines::reqwest_engine::ReqwestEngine;
use crate::engines::traits::{ScrapeRequest, ScraperEngine};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

// Use domain services
pub async fn extract(
    Extension(settings): Extension<Arc<Settings>>,
    Json(payload): Json<ExtractRequestDto>,
) -> impl IntoResponse {
    let mut results = Vec::new();

    // In a real production app, these services should be injected via state
    let extraction_service = ExtractionService::new(Box::new(LLMService::new(&settings)));
    let fetch_engine = ReqwestEngine;

    for url in payload.urls {
        // Real scraping using ReqwestEngine
        let scrape_request = ScrapeRequest {
            url: url.clone(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };

        let (html_content, scrape_error) = match fetch_engine.scrape(&scrape_request).await {
            Ok(response) => (response.content, None),
            Err(e) => (String::new(), Some(e.to_string())),
        };

        if let Some(err) = scrape_error {
            results.push(ExtractResultDto {
                url,
                data: serde_json::Value::Null,
                error: Some(format!("Scrape failed: {}", err)),
            });
            continue;
        }

        let result_data = if let Some(prompt) = &payload.prompt {
            // If prompt is provided, we construct a rule that uses LLM
            let mut rules = HashMap::new();
            rules.insert(
                "extraction".to_string(),
                ExtractionRule {
                    selector: None,
                    attr: None,
                    is_array: false,
                    use_llm: Some(true),
                    llm_prompt: Some(prompt.clone()),
                },
            );

            match extraction_service.extract_data(&html_content, &rules).await {
                Ok((val, _)) => val,
                Err(e) => json!({ "error": format!("Extraction failed: {}", e) }),
            }
        } else if let Some(schema) = &payload.schema {
            let mut rules = HashMap::new();
            rules.insert(
                "data".to_string(),
                ExtractionRule {
                    selector: None,
                    attr: None,
                    is_array: false,
                    use_llm: Some(true),
                    llm_prompt: Some(format!("Extract data according to this schema: {}", schema)),
                },
            );

            match extraction_service.extract_data(&html_content, &rules).await {
                Ok((val, _)) => val,
                Err(e) => json!({ "error": format!("Extraction failed: {}", e) }),
            }
        } else {
            json!({ "error": "No prompt or schema provided for extraction" })
        };

        results.push(ExtractResultDto {
            url,
            data: result_data,
            error: None,
        });
    }

    (StatusCode::OK, Json(ExtractResponseDto { results })).into_response()
}
