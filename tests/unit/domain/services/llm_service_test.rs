// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crawlrs::domain::services::llm_service::{LLMService, TokenUsage};
    use serde_json::json;

    #[tokio::test]
    async fn test_extract_data_with_real_implementation() {
        // Test that the LLM service can be created and used
        let service = LLMService::default();
        
        // Test with a simple schema and text
        let text = "The price of the product is $29.99 and it has 4.5 stars rating.";
        let schema = json!({
            "type": "object",
            "properties": {
                "price": {"type": "number"},
                "rating": {"type": "number"}
            },
            "required": ["price", "rating"]
        });

        // This will fail because no API key is configured, but we can test the error handling
        let result = service.extract_data(text, &schema).await;
        
        // Should return an error about missing API key
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("LLM API key not configured"));
    }

    #[test]
    fn test_token_usage_serialization() {
        let usage = TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let deserialized: TokenUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_tokens, 30);
    }
}