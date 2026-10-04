// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! MLLM 引擎配置
//!
//! 定义视觉导航引擎的配置参数。

use serde::{Deserialize, Serialize};

/// MLLM 自主导航爬取引擎配置
///
/// 控制视觉模型驱动的浏览器自主导航行为。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MllmEngineConfig {
    /// 视觉模型名称（如 "gemini-2.0-flash", "gpt-4o"）
    pub vision_model: String,

    /// 最大导航迭代次数（防止无限循环）
    pub max_iterations: u8,

    /// 截图质量 (0-100)，影响截图清晰度和 token 消耗
    pub screenshot_quality: u8,

    /// 系统提示词（指导视觉模型如何分析页面并做出决策）
    pub system_prompt: String,

    /// 最大 token 预算（单次请求）
    pub max_token_budget: u32,

    /// 引擎级最大响应时间（秒）
    pub mrt_seconds: u64,

    /// 混合 DOM+AX 树快照开关（F12）：true 时每轮导航决策附 `[id] role: name`
    /// 文本大纲（`[id]` 为 backendNodeId，可在 Click 决策中以 `ax_id` 引用），
    /// 与截图视觉通道并存；大纲长度受 [`MllmEngineConfig::max_token_budget`] 约束
    pub ax_tree_enabled: bool,
}

impl Default for MllmEngineConfig {
    fn default() -> Self {
        Self {
            vision_model: "gemini-2.0-flash".to_string(),
            max_iterations: 10,
            screenshot_quality: 70,
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            max_token_budget: 4096,
            mrt_seconds: 60,
            ax_tree_enabled: false,
        }
    }
}

/// 默认系统提示词 — 网页导航助手角色
const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a web navigation assistant. Your task is to analyze the current page screenshot and decide the next action to accomplish the user's goal.

Available actions:
- click(selector): Click on an element
- scroll(direction): Scroll the page (up/down)
- input(selector, text): Type text into an input field
- wait(seconds): Wait for page to stabilize
- extract: Extract the current page content
- done: Navigation complete

Respond with a JSON object:
{"action": "<action_name>", "selector": "<css_selector>", "text": "<input_text>", "direction": "<up|down>", "seconds": <number>, "reasoning": "<brief explanation>"}

Only include relevant fields for the chosen action. Be precise with CSS selectors."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MllmEngineConfig::default();
        assert_eq!(config.vision_model, "gemini-2.0-flash");
        assert_eq!(config.max_iterations, 10);
        assert_eq!(config.screenshot_quality, 70);
        assert_eq!(config.max_token_budget, 4096);
        assert_eq!(config.mrt_seconds, 60);
        assert!(!config.ax_tree_enabled, "AX 树快照默认关（F12）");
        assert!(!config.system_prompt.is_empty());
    }

    #[test]
    fn test_config_serialization() {
        let config = MllmEngineConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: MllmEngineConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.vision_model, config.vision_model);
        assert_eq!(deserialized.max_iterations, config.max_iterations);
        assert_eq!(deserialized.ax_tree_enabled, config.ax_tree_enabled);
    }

    /// AX 树快照开关反序列化（F12；serde 按既有惯例全键必填）
    #[test]
    fn test_config_ax_tree_enabled_parses() {
        let config: MllmEngineConfig = serde_json::from_str(
            r#"{"vision_model": "m", "max_iterations": 3, "screenshot_quality": 70,
                "system_prompt": "s", "max_token_budget": 2048, "mrt_seconds": 60,
                "ax_tree_enabled": true}"#,
        )
        .unwrap();
        assert!(config.ax_tree_enabled);
    }
}
