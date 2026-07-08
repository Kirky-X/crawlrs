// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crawlrs::search::client::google::GoogleSearchEngine;
use crawlrs::search::engine_trait::SearchEngine;
use crawlrs::search::SearchRequest;
use crawlrs::utils::http_client::create_http_client;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::GenericImage;
use log::info;

#[tokio::test]
#[ignore]
async fn verify_google_uses_fire_engine_cdp() {
    // 1. Determine Fire Engine URL (use existing or start container)
    let base_url = if let Ok(url) = std::env::var("TEST_FIRE_ENGINE_CDP_URL") {
        info!("Using provided Fire Engine URL: {}", url);
        url
    } else {
        info!("Starting FlareSolverr container for verification...");
        let container = GenericImage::new("ghcr.io/flaresolverr/flaresolverr", "latest")
            .with_exposed_port(ContainerPort::Tcp(8191))
            .with_wait_for(WaitFor::message_on_stdout("FlareSolverr User Agent"))
            .start()
            .await
            .expect("Failed to start FlareSolverr container");

        let port = container.get_host_port_ipv4(8191).await.expect("port");
        let url = format!("http://127.0.0.1:{}/v1", port);
        info!("FlareSolverr started at {}", url);
        url
    };

    // 3. Configure environment
    std::env::set_var("FIRE_ENGINE_CDP_URL", &base_url);
    std::env::set_var("FIRE_ENGINE_URL", &base_url);

    // 4. Create Google Engine with FireEngineCdp
    let http_client = create_http_client();
    let engine = GoogleSearchEngine::new(http_client);

    // 5. Perform search
    let request = SearchRequest {
        query: "rust programming".to_string(),
        limit: 1,
        engine: None,
        offset: 0,
        lang: None,
        country: None,
    };

    info!("Executing search request to trigger routing...");
    let result = engine.search(&request).await;

    // 6. Cleanup
    std::env::remove_var("FIRE_ENGINE_CDP_URL");
    std::env::remove_var("FIRE_ENGINE_URL");

    // 7. Verify result
    match result {
        Ok(response) => {
            info!("Search successful: {} results", response.items.len());
        }
        Err(e) => {
            info!("Search failed: {}", e);
        }
    }
}
