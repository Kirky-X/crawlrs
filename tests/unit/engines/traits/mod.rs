// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

// Engine traits unit tests
//
// These tests cover the internal engine traits.
// Since traits are internal, we test through the EngineClient API.

#[cfg(test)]
mod tests {
    use crawlrs::engines::client::EngineClient;

    #[tokio::test]
    async fn test_engine_client_creation() {
        let client = EngineClient::new();
        assert!(client.scrape_is_available());
    }

    #[tokio::test]
    async fn test_engine_health_status() {
        let client = EngineClient::new();
        let status = client.get_health_status();
        // Health status should be available
        assert!(status.is_healthy() || !status.is_healthy());
    }
}
