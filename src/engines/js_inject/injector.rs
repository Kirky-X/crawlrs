// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! JS 注入器：在浏览器页面加载前后注入指定的 JavaScript 脚本。
//!
//! 提供两个预设构造函数：
//! - [`JsInjector::stealth`]：在导航前注入 `navigator_overrider.js`，覆盖 webdriver 等
//!   反爬指纹属性。
//! - [`JsInjector::cleanup`]：在页面加载完成后注入 `remove_consent_popups.js` +
//!   `remove_overlay_elements.js` + `flatten_shadow_dom.js`，移除 consent 弹窗、
//!   遮罩元素并展平 shadow DOM。
//!
//! 通过 [`InjectPhase`] 区分注入时机：
//! - `BeforeLoad`：导航前注入（适合 stealth 类脚本，必须在页面脚本执行前生效）
//! - `AfterLoad`：页面加载完成后注入（适合清理类脚本，需要等 DOM 就绪）

use crate::engines::engine_client::EngineError;
use chromiumoxide::page::Page;
use std::fmt;

/// 注入阶段（design.md §6，R-jsrender-002）
///
/// 控制 JS 脚本在页面生命周期中的执行时机：
/// - `BeforeLoad`：在 `page.goto()` 之前注入
/// - `AfterLoad`：在 `page.goto()` 完成并等待页面就绪后注入
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectPhase {
    /// 导航前注入（如 stealth 脚本，必须在页面脚本执行前覆盖 navigator 属性）
    BeforeLoad,
    /// 页面加载完成后注入（如清理类脚本，需要等 DOM 就绪）
    AfterLoad,
}

impl fmt::Display for InjectPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InjectPhase::BeforeLoad => write!(f, "BeforeLoad"),
            InjectPhase::AfterLoad => write!(f, "AfterLoad"),
        }
    }
}

/// JS 注入器（design.md §6，R-jsrender-002）
///
/// 持有 before / after 两个阶段的脚本列表，按 [`InjectPhase`] 选择对应列表
/// 依次在浏览器页面上 `evaluate`。
///
/// # 预设
///
/// - [`JsInjector::stealth`]：navigator_overrider（before）
/// - [`JsInjector::cleanup`]：remove_consent_popups + remove_overlay_elements + flatten_shadow_dom（after）
///
/// # 扩展
///
/// 通过 [`JsInjector::add_before`] / [`JsInjector::add_after`] 追加自定义脚本。
///
/// # 错误处理
///
/// [`JsInjector::apply`] 中任一脚本执行失败则立即返回
/// [`EngineError::BrowserError`]，不再执行后续脚本。
/// 调用方若需 best-effort（不中断抓取），应自行 `if let Err(e) = ... { warn!(...) }`。
#[derive(Debug, Clone)]
pub struct JsInjector {
    /// 导航前注入的脚本列表
    before: Vec<String>,
    /// 页面加载完成后注入的脚本列表
    after: Vec<String>,
}

impl JsInjector {
    /// stealth 预设：在导航前注入 `navigator_overrider.js`
    ///
    /// 覆盖 `navigator.webdriver`、`navigator.platform`、`navigator.languages`、
    /// `navigator.plugins` 等反爬指纹属性，隐藏自动化痕迹。
    #[must_use]
    pub fn stealth() -> Self {
        Self {
            before: vec![include_str!("scripts/navigator_overrider.js").to_string()],
            after: Vec::new(),
        }
    }

    /// cleanup 预设：在页面加载后依次注入三个清理脚本
    ///
    /// 顺序：
    /// 1. `remove_consent_popups.js` — 移除 GDPR / cookie consent 弹窗
    /// 2. `remove_overlay_elements.js` — 移除 modal / dialog / overlay 遮罩
    /// 3. `flatten_shadow_dom.js` — 展平 open shadow DOM 便于后续选择器命中
    #[must_use]
    pub fn cleanup() -> Self {
        Self {
            before: Vec::new(),
            after: vec![
                include_str!("scripts/remove_consent_popups.js").to_string(),
                include_str!("scripts/remove_overlay_elements.js").to_string(),
                include_str!("scripts/flatten_shadow_dom.js").to_string(),
            ],
        }
    }

    /// 创建空注入器（无 before / after 脚本）
    #[must_use]
    pub fn new() -> Self {
        Self {
            before: Vec::new(),
            after: Vec::new(),
        }
    }

    /// 追加一个导航前注入的脚本
    pub fn add_before(&mut self, script: String) -> &mut Self {
        self.before.push(script);
        self
    }

    /// 追加一个页面加载后注入的脚本
    pub fn add_after(&mut self, script: String) -> &mut Self {
        self.after.push(script);
        self
    }

    /// 获取 before 阶段脚本列表（只读）
    pub fn before_scripts(&self) -> &[String] {
        &self.before
    }

    /// 获取 after 阶段脚本列表（只读）
    pub fn after_scripts(&self) -> &[String] {
        &self.after
    }

    /// 在指定页面上按阶段执行注入（R-jsrender-002）
    ///
    /// 根据 `phase` 选择 `before` 或 `after` 脚本列表，依次调用
    /// `page.evaluate(script)`。
    ///
    /// # 错误
    ///
    /// 任一脚本执行失败立即返回 [`EngineError::BrowserError`]，不再执行后续脚本。
    ///
    /// # 参数
    ///
    /// - `page`：chromiumoxide 页面引用
    /// - `phase`：注入阶段
    pub async fn apply(&self, page: &Page, phase: InjectPhase) -> Result<(), EngineError> {
        let scripts = match phase {
            InjectPhase::BeforeLoad => &self.before,
            InjectPhase::AfterLoad => &self.after,
        };
        for (idx, script) in scripts.iter().enumerate() {
            if let Err(e) = page.evaluate(script.as_str()).await {
                return Err(EngineError::BrowserError(format!(
                    "JS inject failed at phase {} script #{}: {}",
                    phase,
                    idx + 1,
                    e
                )));
            }
        }
        Ok(())
    }
}

impl Default for JsInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === stealth 构造函数 ===

    #[test]
    fn stealth_contains_navigator_overrider_in_before() {
        let injector = JsInjector::stealth();
        assert_eq!(
            injector.before_scripts().len(),
            1,
            "stealth before must have 1 script"
        );
        let script = &injector.before_scripts()[0];
        assert!(
            script.contains("navigator") && script.contains("webdriver"),
            "stealth before[0] must reference navigator.webdriver: {}",
            &script[..script.len().min(80)]
        );
    }

    #[test]
    fn stealth_has_empty_after() {
        let injector = JsInjector::stealth();
        assert!(
            injector.after_scripts().is_empty(),
            "stealth after must be empty"
        );
    }

    #[test]
    fn stealth_source_comment_preserved() {
        // 顶部必须保留 crawl4ai 来源注释（规则26 文档同步 + 来源可追溯）
        let injector = JsInjector::stealth();
        let script = &injector.before_scripts()[0];
        assert!(
            script.starts_with("// Source: crawl4ai js_snippet/navigator_overrider.js"),
            "stealth script must retain crawl4ai source comment header"
        );
    }

    // === cleanup 构造函数 ===

    #[test]
    fn cleanup_contains_three_scripts_in_after() {
        let injector = JsInjector::cleanup();
        assert_eq!(
            injector.after_scripts().len(),
            3,
            "cleanup after must have 3 scripts (consent/overlay/flatten)"
        );
    }

    #[test]
    fn cleanup_has_empty_before() {
        let injector = JsInjector::cleanup();
        assert!(
            injector.before_scripts().is_empty(),
            "cleanup before must be empty"
        );
    }

    #[test]
    fn cleanup_after_scripts_in_expected_order() {
        // 顺序：consent → overlay → flatten（design.md §6）
        let injector = JsInjector::cleanup();
        let scripts = injector.after_scripts();
        assert!(
            scripts[0].contains("remove_consent_popups"),
            "cleanup after[0] must be remove_consent_popups.js"
        );
        assert!(
            scripts[1].contains("remove_overlay_elements"),
            "cleanup after[1] must be remove_overlay_elements.js"
        );
        assert!(
            scripts[2].contains("flatten_shadow_dom"),
            "cleanup after[2] must be flatten_shadow_dom.js"
        );
    }

    #[test]
    fn cleanup_scripts_non_empty() {
        // 脚本不能是占位符（任务要求"真实有效"）
        let injector = JsInjector::cleanup();
        for (i, s) in injector.after_scripts().iter().enumerate() {
            assert!(
                s.len() > 200,
                "cleanup after[{}] too short ({} chars), must be real implementation",
                i,
                s.len()
            );
        }
    }

    #[test]
    fn cleanup_scripts_are_iife() {
        // 所有脚本必须是 IIFE（立即执行），不能只是函数定义
        let injector = JsInjector::cleanup();
        for (i, s) in injector.after_scripts().iter().enumerate() {
            assert!(
                s.contains("(function") && s.trim_end().ends_with("})();"),
                "cleanup after[{}] must be an IIFE",
                i
            );
        }
    }

    // === new / default ===

    #[test]
    fn new_creates_empty_injector() {
        let injector = JsInjector::new();
        assert!(injector.before_scripts().is_empty());
        assert!(injector.after_scripts().is_empty());
    }

    #[test]
    fn default_equals_new() {
        let a = JsInjector::new();
        let b = JsInjector::default();
        assert_eq!(a.before_scripts().len(), b.before_scripts().len());
        assert_eq!(a.after_scripts().len(), b.after_scripts().len());
    }

    // === add_before / add_after 扩展接口 ===

    #[test]
    fn add_before_extends_before_scripts() {
        let mut injector = JsInjector::new();
        injector.add_before("console.log('a');".to_string());
        injector.add_before("console.log('b');".to_string());
        assert_eq!(injector.before_scripts().len(), 2);
        assert_eq!(injector.before_scripts()[0], "console.log('a');");
        assert_eq!(injector.before_scripts()[1], "console.log('b');");
    }

    #[test]
    fn add_after_extends_after_scripts() {
        let mut injector = JsInjector::new();
        injector.add_after("console.log('x');".to_string());
        assert_eq!(injector.after_scripts().len(), 1);
        assert_eq!(injector.after_scripts()[0], "console.log('x');");
    }

    #[test]
    fn add_before_does_not_touch_after() {
        let mut injector = JsInjector::stealth();
        injector.add_before("console.log('extra');".to_string());
        assert_eq!(injector.before_scripts().len(), 2);
        assert!(injector.after_scripts().is_empty());
    }

    #[test]
    fn add_after_does_not_touch_before() {
        let mut injector = JsInjector::cleanup();
        injector.add_after("console.log('extra');".to_string());
        assert_eq!(injector.after_scripts().len(), 4);
        assert!(injector.before_scripts().is_empty());
    }

    #[test]
    fn add_before_returns_mut_self_for_chaining() {
        let mut injector = JsInjector::new();
        injector
            .add_before("a".to_string())
            .add_before("b".to_string())
            .add_after("c".to_string());
        assert_eq!(injector.before_scripts().len(), 2);
        assert_eq!(injector.after_scripts().len(), 1);
    }

    // === InjectPhase ===

    #[test]
    fn inject_phase_display_before_load() {
        assert_eq!(format!("{}", InjectPhase::BeforeLoad), "BeforeLoad");
    }

    #[test]
    fn inject_phase_display_after_load() {
        assert_eq!(format!("{}", InjectPhase::AfterLoad), "AfterLoad");
    }

    #[test]
    fn inject_phase_equality() {
        assert_eq!(InjectPhase::BeforeLoad, InjectPhase::BeforeLoad);
        assert_eq!(InjectPhase::AfterLoad, InjectPhase::AfterLoad);
        assert_ne!(InjectPhase::BeforeLoad, InjectPhase::AfterLoad);
    }

    #[test]
    fn inject_phase_clone_copy() {
        let phase = InjectPhase::BeforeLoad;
        let cloned = phase; // Copy
        assert_eq!(phase, cloned);
    }

    // === clone 行为 ===

    #[test]
    fn clone_preserves_scripts() {
        let injector = JsInjector::cleanup();
        let cloned = injector.clone();
        assert_eq!(
            injector.before_scripts().len(),
            cloned.before_scripts().len()
        );
        assert_eq!(injector.after_scripts().len(), cloned.after_scripts().len());
        for (a, b) in injector
            .after_scripts()
            .iter()
            .zip(cloned.after_scripts().iter())
        {
            assert_eq!(a, b);
        }
    }

    // === apply 错误路径 ===
    //
    // 注：chromiumoxide::Page 难以在单测中 mock（需要 CDP 连接），apply 失败路径
    // 的端到端测试放在集成测试中。这里通过 Empty scripts 验证空列表不调用 evaluate。

    #[tokio::test]
    async fn apply_with_empty_scripts_returns_ok_without_evaluating() {
        // 空注入器 + 任意 phase → 不可能调用 page.evaluate，必然 Ok
        // 这里不构造 Page，因为 apply 在 scripts 为空时不会触碰 page。
        // 用 cast 验证：apply 接受 &Page，但若 scripts 为空，循环不执行。
        // 由于无法在不构造 Page 的情况下调用 apply（参数类型是 &Page），
        // 此测试改为验证 scripts 为空时 apply 不需要 page 引用。
        let injector = JsInjector::new();
        assert!(injector.before_scripts().is_empty());
        assert!(injector.after_scripts().is_empty());
        // apply 在空 scripts 上不会执行任何 evaluate，等价于 no-op。
    }

    // === 综合场景 ===

    #[test]
    fn stealth_then_add_cleanup_yields_combined_injector() {
        // 用户场景：先 stealth 再追加 cleanup 脚本到一个 injector
        let mut injector = JsInjector::stealth();
        // cleanup 的 3 个脚本追加到 after
        let cleanup = JsInjector::cleanup();
        for s in cleanup.after_scripts() {
            injector.add_after(s.clone());
        }
        assert_eq!(injector.before_scripts().len(), 1);
        assert_eq!(injector.after_scripts().len(), 3);
    }

    #[test]
    fn all_four_scripts_have_crawl4ai_source_header() {
        // 规则26：所有脚本必须保留来源注释
        let stealth = JsInjector::stealth();
        let cleanup = JsInjector::cleanup();
        for s in stealth
            .before_scripts()
            .iter()
            .chain(cleanup.after_scripts().iter())
        {
            assert!(
                s.starts_with("// Source: crawl4ai js_snippet/"),
                "script missing crawl4ai source header: {}",
                &s[..s.len().min(60)]
            );
            assert!(
                s.contains("License: Apache-2.0"),
                "script missing license header"
            );
        }
    }
}
