// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0
#![cfg(test)]
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crawlrs::domain::services::llm::TokenUsage;

    #[test]
    fn test_token_usage_serialization() {
        let usage = TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_string(&usage).expect("Failed to serialize token usage");
        let deserialized: TokenUsage =
            serde_json::from_str(&json).expect("Failed to deserialize token usage");
        assert_eq!(deserialized.total_tokens, 30);
    }
}
