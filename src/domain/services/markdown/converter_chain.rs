// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 转换器链执行器：primary（moli-html2md）→ fallback（htmd）
//!
//! 兜底触发条件（设计 D4，两条）：
//! 1. `primary.convert` 返回 `Err`；
//! 2. 输入含可见文本（body 子树存在非空白文本，跳过 script/style/template）
//!    而 primary 输出 trim 后为空（保真异常的防呆；纯脚本/空壳页面的空输出
//!    是正确抑制，不触发兜底）。
//!
//! 触发时产生 `log::warn!` 与失败计数（metrics feature：
//! `crawlrs_md_converter_fallback_total{from,to,reason}`）。
//! 双转换器均失败 → [`MarkdownError::ChainFailed`]（携带两侧原因），
//! 绝不返回空字符串假装成功。

use crate::domain::services::markdown_service::{MarkdownConverter, MarkdownError};

/// 主转 htmd 兜底的转换器链
pub struct ConverterChain {
    primary: Box<dyn MarkdownConverter>,
    fallback: Box<dyn MarkdownConverter>,
}

impl ConverterChain {
    fn new(primary: Box<dyn MarkdownConverter>, fallback: Box<dyn MarkdownConverter>) -> Self {
        Self { primary, fallback }
    }

    /// 默认链：moli-html2md 主 + htmd 兜底
    pub fn default_chain() -> Self {
        Self::new(
            Box::new(
                crate::domain::services::markdown::moli_converter::MoliHtml2MdConverter::new(),
            ),
            Box::new(crate::domain::services::markdown_service::HtmdMarkdownService::new()),
        )
    }

    /// 执行主转换器；返回 `Err(reason)` 表示应触发兜底。
    ///
    /// `Ok` 既包含成功产物，也包含「无可见文本的空输出」——后者是正确抑制，
    /// 直接接受，不触发兜底。
    fn try_primary(&self, html: &str) -> Result<String, String> {
        match self.primary.convert(html) {
            Ok(md) => {
                if md.trim().is_empty() {
                    // 防呆仅对「输入含可见文本」生效：moli 对纯脚本/样式页、
                    // title-only 壳、SPA 空壳输出空串是正确抑制——此时兜底会让
                    // htmd 把 script 内 JSON（如 __NEXT_DATA__）当正文灌入产物
                    // （三维审查 A1）。无可见文本 → 空串是对的，直接接受。
                    if !has_visible_text(html) {
                        return Ok(md);
                    }
                    record_fallback(self.primary.name(), self.fallback.name(), "empty_output");
                    Err("empty_output".to_string())
                } else {
                    Ok(md)
                }
            }
            Err(e) => {
                record_fallback(self.primary.name(), self.fallback.name(), "error");
                Err(format!("{}", e))
            }
        }
    }
}

/// 输入 HTML 是否含可见文本（决定空输出是否异常、兜底是否该触发）。
///
/// 从 `<body>` 子树扫描（head 内 title 不算可见正文），跳过
/// script/style/template/noscript；存在非空白 Text 即视为可见内容。
/// **显式栈迭代**（与 moli 遍历同款）：DOM 深度不得进入 Rust 调用栈——
/// 递归实现会在 2MiB 栈上被万级嵌套（release 实测 20000 层）击穿并 abort 进程
/// （架构复核 HIGH 实证）。仅在 primary 输出空白的罕见路径调用。
fn has_visible_text(html: &str) -> bool {
    use scraper::node::Node;

    let doc = scraper::Html::parse_document(html);
    let root = doc.tree.root();
    let body = root
        .descendants()
        .find(|n| matches!(n.value(), Node::Element(e) if e.name() == "body"));
    let start = body.unwrap_or(root);
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        match node.value() {
            Node::Text(t) => {
                if !t.text.trim().is_empty() {
                    return true;
                }
            }
            Node::Element(el) => {
                const SKIP: [&str; 4] = ["script", "style", "template", "noscript"];
                if SKIP.contains(&el.name()) {
                    continue;
                }
                stack.extend(node.children());
            }
            Node::Document | Node::Fragment => stack.extend(node.children()),
            _ => {}
        }
    }
    false
}

fn record_fallback(from: &str, to: &str, reason: &str) {
    log::warn!(
        target: "crawlrs::markdown",
        "markdown converter fallback triggered: {} -> {} (reason: {})",
        from,
        to,
        reason
    );
    #[cfg(feature = "metrics")]
    metrics::counter!(
        "crawlrs_md_converter_fallback_total",
        "from" => from.to_string(),
        "to" => to.to_string(),
        "reason" => reason.to_string()
    )
    .increment(1);
}

impl MarkdownConverter for ConverterChain {
    fn name(&self) -> &'static str {
        "moli+htmd"
    }

    fn convert(&self, html: &str) -> Result<String, MarkdownError> {
        let primary_reason: String = match self.try_primary(html) {
            Ok(md) => return Ok(md),
            Err(reason) => reason,
        };

        match self.fallback.convert(html) {
            Ok(md) => Ok(md),
            Err(fallback_err) => Err(MarkdownError::ChainFailed {
                primary: primary_reason,
                fallback: format!("{}", fallback_err),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试桩：固定返回或固定错误
    struct StubConverter {
        name: &'static str,
        result: Result<&'static str, &'static str>,
    }

    impl MarkdownConverter for StubConverter {
        fn name(&self) -> &'static str {
            self.name
        }

        fn convert(&self, _html: &str) -> Result<String, MarkdownError> {
            self.result
                .map(str::to_string)
                .map_err(|e| MarkdownError::ConversionFailed(e.to_string()))
        }
    }

    fn chain(
        primary: Result<&'static str, &'static str>,
        fallback: Result<&'static str, &'static str>,
    ) -> ConverterChain {
        ConverterChain::new(
            Box::new(StubConverter {
                name: "moli",
                result: primary,
            }),
            Box::new(StubConverter {
                name: "htmd",
                result: fallback,
            }),
        )
    }

    /// 分支 1：primary 成功 → 直接返回，兜底不参与
    #[test]
    fn primary_success_passthrough() {
        let c = chain(Ok("primary-md"), Ok("fallback-md"));
        assert_eq!(c.convert("<p>x</p>").unwrap(), "primary-md");
    }

    /// 分支 2：primary Err → fallback 输出
    #[test]
    fn primary_error_falls_back() {
        let c = chain(Err("boom"), Ok("fallback-md"));
        assert_eq!(c.convert("<p>x</p>").unwrap(), "fallback-md");
    }

    /// 分支 3：输入含可见文本而 primary 输出空白 → 防呆走兜底
    #[test]
    fn primary_empty_output_falls_back() {
        let c = chain(Ok("   "), Ok("fallback-md"));
        assert_eq!(c.convert("<p>x</p>").unwrap(), "fallback-md");
    }

    /// 空输入下 primary 空输出是正常语义，不触发兜底
    #[test]
    fn empty_input_does_not_trigger_fallback() {
        struct CountingConverter;
        impl MarkdownConverter for CountingConverter {
            fn name(&self) -> &'static str {
                "counting"
            }
            fn convert(&self, _html: &str) -> Result<String, MarkdownError> {
                Ok(String::new())
            }
        }
        let c = ConverterChain::new(
            Box::new(CountingConverter),
            Box::new(StubConverter {
                name: "htmd",
                result: Ok("should-not-run"),
            }),
        );
        assert_eq!(c.convert("   \n ").unwrap(), "");
    }

    /// 分支 4：双失败 → ChainFailed 携带两侧原因
    #[test]
    fn both_fail_returns_chain_error_with_both_reasons() {
        let c = chain(Err("primary-broke"), Err("fallback-broke"));
        match c.convert("<p>x</p>").unwrap_err() {
            MarkdownError::ChainFailed { primary, fallback } => {
                assert!(
                    primary.contains("primary-broke"),
                    "primary 原因缺失: {primary}"
                );
                assert!(
                    fallback.contains("fallback-broke"),
                    "fallback 原因缺失: {fallback}"
                );
            }
            other => panic!("期望 ChainFailed，实际 {other:?}"),
        }
    }

    /// A1 钉测：纯脚本页（无可见文本）primary 空输出 → 接受空串，不触发兜底
    /// （回归防护：禁止把 script 内 JSON 用 htmd 灌回产物）
    #[test]
    fn empty_output_without_visible_text_is_accepted() {
        struct EmptyOk;
        impl MarkdownConverter for EmptyOk {
            fn name(&self) -> &'static str {
                "stub-moli"
            }
            fn convert(&self, _html: &str) -> Result<String, MarkdownError> {
                Ok(String::new())
            }
        }
        let c = ConverterChain::new(
            Box::new(EmptyOk),
            Box::new(StubConverter {
                name: "htmd",
                result: Ok("json-should-not-appear"),
            }),
        );
        // 纯脚本页：无可见文本 → 空串被接受
        let html = r#"<html><body><script>var a=1;</script></body></html>"#;
        assert_eq!(c.convert(html).unwrap(), "");
        // title-only 壳（head 内文本不算可见正文）
        let html = r#"<html><head><title>Only Title</title></head><body></body></html>"#;
        assert_eq!(c.convert(html).unwrap(), "");
        // 有可见文本而 primary 空输出 → 兜底生效（防呆保留）
        let html = r#"<html><body><p>real content</p></body></html>"#;
        assert_eq!(c.convert(html).unwrap(), "json-should-not-appear");
    }

    /// 深嵌套无文本输入不得栈溢出（架构复核 HIGH 回归）。
    ///
    /// 递归实现在 2MiB 栈上约 1-2 万层才溢出——5000 层递归仍可通过，直接用默认
    /// 线程抓不住回归。故在 **256KiB 小栈线程** 内跑 5000 层：递归实现此处必溢出
    /// abort（测试进程崩溃即 CI 红），迭代实现必须返回 Ok("")。深度取 5000 是为
    /// 控制 html5ever 解析的二次方开销（20000 层 debug 实测 12.9s）。
    #[test]
    fn deep_nested_no_text_input_does_not_overflow() {
        let depth = 5_000;
        let mut html = String::from("<html><body>");
        html.push_str(&"<div>".repeat(depth));
        html.push_str(&"</div>".repeat(depth));
        html.push_str("</body></html>");
        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                struct EmptyOk;
                impl MarkdownConverter for EmptyOk {
                    fn name(&self) -> &'static str {
                        "stub-moli"
                    }
                    fn convert(&self, _html: &str) -> Result<String, MarkdownError> {
                        Ok(String::new())
                    }
                }
                let c = ConverterChain::new(
                    Box::new(EmptyOk),
                    Box::new(StubConverter {
                        name: "htmd",
                        result: Ok("should-not-run"),
                    }),
                );
                c.convert(&html)
            })
            .expect("spawn small-stack thread");
        let md = handle.join().expect("small-stack thread panicked");
        assert_eq!(md.unwrap(), "", "深层无文本页应接受空输出");
    }

    /// 默认链的 name 标识
    #[test]
    fn default_chain_name() {
        assert_eq!(ConverterChain::default_chain().name(), "moli+htmd");
    }
}
