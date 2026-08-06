// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! T047: MLLM 导航决策类型
//!
//! 定义视觉模型输出的结构化决策枚举和 JSON 解析器。

use serde::{Deserialize, Serialize};

/// MLLM 导航决策 — 视觉模型分析截图后输出的下一步操作
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MllmDecision {
    /// 点击页面元素
    Click {
        selector: String,
        #[serde(default)]
        reasoning: String,
    },
    /// 滚动页面
    Scroll {
        direction: ScrollDirection,
        #[serde(default)]
        reasoning: String,
    },
    /// 在输入框中输入文本
    Input {
        selector: String,
        text: String,
        #[serde(default)]
        reasoning: String,
    },
    /// 等待页面稳定
    Wait {
        #[serde(default = "default_wait_seconds")]
        seconds: u32,
        #[serde(default)]
        reasoning: String,
    },
    /// 提取当前页面内容
    Extract {
        #[serde(default)]
        reasoning: String,
    },
    /// 导航完成
    Done {
        #[serde(default)]
        reasoning: String,
    },
}

/// 滚动方向
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollDirection {
    Up,
    Down,
}

fn default_wait_seconds() -> u32 {
    2
}

/// 解析视觉模型返回的 JSON 为 `MllmDecision`
///
/// # 参数
///
/// * `json` - 视觉模型返回的 JSON 字符串
///
/// # 返回值
///
/// * `Ok(MllmDecision)` - 成功解析为决策
/// * `Err(String)` - 解析失败，包含错误描述
pub fn parse_decision(json: &str) -> Result<MllmDecision, String> {
    // 尝试提取 JSON 块（模型可能在 JSON 外包裹 markdown 代码块或额外文本）
    let json_str = extract_json(json);
    serde_json::from_str::<MllmDecision>(&json_str).map_err(|e| {
        format!(
            "Failed to parse MLLM decision JSON: {} (input: {})",
            e,
            json_str.chars().take(200).collect::<String>()
        )
    })
}

/// 从可能包含 markdown 代码块或额外文本的响应中提取 JSON
fn extract_json(response: &str) -> String {
    let trimmed = response.trim();

    // 尝试直接解析
    if trimmed.starts_with('{') {
        return trimmed.to_string();
    }

    // 尝试从 markdown 代码块中提取
    if let Some(start) = trimmed.find("```json") {
        let after_marker = &trimmed[start + 7..];
        if let Some(end) = after_marker.find("```") {
            return after_marker[..end].trim().to_string();
        }
    }

    // 尝试从 ``` 代码块中提取
    if let Some(start) = trimmed.find("```") {
        let after_marker = &trimmed[start + 3..];
        // 跳过可能的语言标识符行
        let json_start = after_marker.find('\n').map(|i| &after_marker[i + 1..]).unwrap_or(after_marker);
        if let Some(end) = json_start.rfind("```") {
            return json_start[..end].trim().to_string();
        }
    }

    // 尝试找到第一个 { 和最后一个 }
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            return trimmed[start..=end].to_string();
        }
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_click() {
        let json = r#"{"action": "click", "selector": "#submit-btn", "reasoning": "Click submit"}"#;
        let decision = parse_decision(json).unwrap();
        assert!(matches!(decision, MllmDecision::Click { .. }));
        if let MllmDecision::Click { selector, reasoning } = decision {
            assert_eq!(selector, "#submit-btn");
            assert_eq!(reasoning, "Click submit");
        }
    }

    #[test]
    fn test_parse_scroll() {
        let json = r#"{"action": "scroll", "direction": "down"}"#;
        let decision = parse_decision(json).unwrap();
        assert!(matches!(
            decision,
            MllmDecision::Scroll {
                direction: ScrollDirection::Down,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_input() {
        let json = r#"{"action": "input", "selector": "input[name=q]", "text": "rust crawler"}"#;
        let decision = parse_decision(json).unwrap();
        if let MllmDecision::Input { selector, text, .. } = decision {
            assert_eq!(selector, "input[name=q]");
            assert_eq!(text, "rust crawler");
        } else {
            panic!("Expected Input decision");
        }
    }

    #[test]
    fn test_parse_wait() {
        let json = r#"{"action": "wait", "seconds": 3}"#;
        let decision = parse_decision(json).unwrap();
        if let MllmDecision::Wait { seconds, .. } = decision {
            assert_eq!(seconds, 3);
        } else {
            panic!("Expected Wait decision");
        }
    }

    #[test]
    fn test_parse_extract() {
        let json = r#"{"action": "extract"}"#;
        let decision = parse_decision(json).unwrap();
        assert!(matches!(decision, MllmDecision::Extract { .. }));
    }

    #[test]
    fn test_parse_done() {
        let json = r#"{"action": "done", "reasoning": "Found the target content"}"#;
        let decision = parse_decision(json).unwrap();
        assert!(matches!(decision, MllmDecision::Done { .. }));
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_decision("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_markdown_wrapped() {
        let response = r#"Here's my decision:
```json
{"action": "click", "selector": ".next-page"}
```
"#;
        let decision = parse_decision(response).unwrap();
        assert!(matches!(decision, MllmDecision::Click { .. }));
    }

    #[test]
    fn test_parse_with_surrounding_text() {
        let response = r#"Based on the screenshot, I think we should proceed.
{"action": "scroll", "direction": "up", "reasoning": "Need to see header"}
Let me know if you need anything else."#;
        let decision = parse_decision(response).unwrap();
        assert!(matches!(
            decision,
            MllmDecision::Scroll {
                direction: ScrollDirection::Up,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_unknown_action() {
        let json = r#"{"action": "fly"}"#;
        let result = parse_decision(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_scroll_direction_serialization() {
        let up = ScrollDirection::Up;
        let json = serde_json::to_string(&up).unwrap();
        assert_eq!(json, "\"up\"");

        let down = ScrollDirection::Down;
        let json = serde_json::to_string(&down).unwrap();
        assert_eq!(json, "\"down\"");
    }

    #[test]
    fn test_decision_roundtrip() {
        let decisions = vec![
            MllmDecision::Click {
                selector: "#btn".to_string(),
                reasoning: "test".to_string(),
            },
            MllmDecision::Scroll {
                direction: ScrollDirection::Down,
                reasoning: String::new(),
            },
            MllmDecision::Input {
                selector: "input".to_string(),
                text: "hello".to_string(),
                reasoning: String::new(),
            },
            MllmDecision::Wait {
                seconds: 5,
                reasoning: String::new(),
            },
            MllmDecision::Extract {
                reasoning: "got it".to_string(),
            },
            MllmDecision::Done {
                reasoning: String::new(),
            },
        ];

        for decision in decisions {
            let json = serde_json::to_string(&decision).unwrap();
            let parsed: MllmDecision = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, decision);
        }
    }
}
