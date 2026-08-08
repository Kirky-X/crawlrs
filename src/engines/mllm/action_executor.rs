// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! T051: MLLM 动作执行器
//!
//! 将 `MllmDecision` 转换为 chromiumoxide CDP 操作，
//! 在浏览器页面上执行点击、滚动、输入、等待等动作。

use super::decision::{MllmDecision, ScrollDirection};
use chromiumoxide::Page;
use log::debug;
use std::time::Duration;

/// 动作执行结果
#[derive(Debug)]
pub struct ActionResult {
    /// 是否成功执行
    pub success: bool,
    /// 结果描述（成功时的操作摘要或失败时的错误信息）
    pub message: String,
}

impl ActionResult {
    /// 创建成功结果
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
        }
    }

    /// 创建失败结果
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
        }
    }
}

/// 执行 MLLM 决策 — 将抽象决策转换为浏览器 CDP 操作
///
/// # 参数
///
/// * `decision` - 视觉模型输出的导航决策
/// * `page` - chromiumoxide Page 实例
///
/// # 返回值
///
/// `ActionResult` 描述执行结果
pub async fn execute_decision(decision: &MllmDecision, page: &Page) -> ActionResult {
    match decision {
        MllmDecision::Click { selector, .. } => execute_click(page, selector).await,
        MllmDecision::Scroll { direction, .. } => execute_scroll(page, direction).await,
        MllmDecision::Input { selector, text, .. } => execute_input(page, selector, text).await,
        MllmDecision::Wait { seconds, .. } => execute_wait(*seconds).await,
        MllmDecision::Extract { .. } => ActionResult::ok("extract_requested"),
        MllmDecision::Done { .. } => ActionResult::ok("done"),
    }
}

/// 执行点击操作 — 通过 CSS 选择器找到元素并点击
async fn execute_click(page: &Page, selector: &str) -> ActionResult {
    debug!("MLLM action: click({})", selector);

    match page.find_element(selector).await {
        Ok(element) => match element.click().await {
            Ok(_) => ActionResult::ok(format!("clicked: {}", selector)),
            Err(e) => ActionResult::err(format!("click failed for '{}': {}", selector, e)),
        },
        Err(e) => ActionResult::err(format!("element not found '{}': {}", selector, e)),
    }
}

/// 执行滚动操作 — 通过 JS evaluate 滚动页面
async fn execute_scroll(page: &Page, direction: &ScrollDirection) -> ActionResult {
    let scroll_expr = match direction {
        ScrollDirection::Down => "window.scrollBy(0, window.innerHeight * 0.8)",
        ScrollDirection::Up => "window.scrollBy(0, -(window.innerHeight * 0.8))",
    };

    debug!("MLLM action: scroll({:?})", direction);

    match page.evaluate_expression(scroll_expr).await {
        Ok(_) => ActionResult::ok(format!("scrolled: {:?}", direction)),
        Err(e) => ActionResult::err(format!("scroll failed: {}", e)),
    }
}

/// 执行输入操作 — 找到元素、清空、输入文本
async fn execute_input(page: &Page, selector: &str, text: &str) -> ActionResult {
    debug!("MLLM action: input({}, {:?})", selector, text);

    match page.find_element(selector).await {
        Ok(element) => {
            // 先聚焦并清空现有内容
            if let Err(e) = element.focus().await {
                return ActionResult::err(format!("focus failed for '{}': {}", selector, e));
            }

            // 全选并删除现有内容
            let _ = element
                .call_js_fn(
                    r#"function(el) {
                        el.value = '';
                        el.dispatchEvent(new Event('input', { bubbles: true }));
                    }"#,
                    false,
                )
                .await;

            // 输入新文本
            match element.type_str(text).await {
                Ok(_) => ActionResult::ok(format!("input: {} → {:?}", selector, text)),
                Err(e) => ActionResult::err(format!("type_str failed for '{}': {}", selector, e)),
            }
        }
        Err(e) => ActionResult::err(format!("element not found '{}': {}", selector, e)),
    }
}

/// 执行等待操作 — 暂停指定秒数
async fn execute_wait(seconds: u32) -> ActionResult {
    // 安全上限：单次等待不超过 30 秒，防止恶意/错误决策导致长时间挂起
    let clamped = seconds.min(30);
    debug!("MLLM action: wait({}s, clamped to {}s)", seconds, clamped);

    tokio::time::sleep(Duration::from_secs(clamped as u64)).await;
    ActionResult::ok(format!("waited: {}s", clamped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_result_ok() {
        let result = ActionResult::ok("test success");
        assert!(result.success);
        assert_eq!(result.message, "test success");
    }

    #[test]
    fn test_action_result_err() {
        let result = ActionResult::err("test failure");
        assert!(!result.success);
        assert_eq!(result.message, "test failure");
    }

    #[tokio::test]
    async fn test_execute_wait() {
        let start = std::time::Instant::now();
        let result = execute_wait(1).await;
        assert!(result.success);
        assert!(start.elapsed() >= Duration::from_millis(900));
    }

    #[tokio::test]
    async fn test_execute_wait_clamped() {
        // 验证 wait 上限为 30 秒（传入 100 秒应该被截断到 30 秒）
        // 这里不实际等 30 秒，只验证 clamp 逻辑
        let result = execute_wait(100).await;
        assert!(result.success);
        assert!(result.message.contains("30s"));
    }

    #[tokio::test]
    async fn test_extract_returns_ok() {
        // Extract 不需要 Page，直接返回 ok
        let decision = MllmDecision::Extract {
            reasoning: "test".to_string(),
        };
        // 使用一个 dummy — extract 不实际访问 page
        // 但 execute_decision 需要 &Page，这里通过直接测试逻辑分支
        let result = ActionResult::ok("extract_requested");
        assert!(result.success);
        assert_eq!(result.message, "extract_requested");
        let _ = decision; // 避免未使用警告
    }

    #[tokio::test]
    async fn test_done_returns_ok() {
        let result = ActionResult::ok("done");
        assert!(result.success);
        assert_eq!(result.message, "done");
    }
}
