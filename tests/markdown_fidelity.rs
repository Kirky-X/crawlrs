//! 保真对照样本测试（html2md-converter-chain T007 / R-markdown-conversion-004）
//!
//! 四类结构：GFM 表格（thead 表头行 / 全 th 首行 / role=presentation 布局表展开 /
//! 复杂表回退 HTML 段）、MathML 原样保留、TeX→`$..$` 包裹、表单默认值输出。
//! 样本对 {alchemize 默认转换器, htmd} 双跑：断言默认转换器的结构特征（探针实测
//! 输出为断言基准），htmd 对照跑通作为诊断臂的活体验证。

#![cfg(feature = "content")]

use std::sync::Arc;

use crawlrs::config::settings::MarkdownSettings;
use crawlrs::domain::services::markdown::build_converter;
use crawlrs::domain::services::markdown_service::{HtmdMarkdownService, MarkdownConverter};

/// 默认装配：alchemize html 引擎（htmd 基础链 + 自研深链）
fn default_converter() -> Arc<dyn MarkdownConverter> {
    build_converter(&MarkdownSettings::default())
}

fn htmd() -> HtmdMarkdownService {
    HtmdMarkdownService::new()
}

// ---- 1. GFM 表格 ----

/// thead 表头行 → GFM 表格（表头行 + 分隔行 + 数据行）
#[test]
fn gfm_table_with_thead_header_row() {
    let html = r#"<table><thead><tr><th>Name</th><th>Age</th></tr></thead><tbody><tr><td>Alice</td><td>30</td></tr></tbody></table>"#;
    let md = default_converter()
        .convert(html)
        .expect("default converter ok");
    assert!(md.contains("| Name | Age |"), "表头行缺失: {md}");
    assert!(md.contains("| --- | --- |"), "分隔行缺失: {md}");
    assert!(md.contains("| Alice | 30 |"), "数据行缺失: {md}");
    // htmd 对照跑通（诊断臂活体验证）
    assert!(htmd().convert(html).is_ok());
}

/// 全 th 首行（无 thead 包裹）→ 首行识别为表头
#[test]
fn gfm_table_all_th_first_row() {
    let html = r#"<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>"#;
    let md = default_converter()
        .convert(html)
        .expect("default converter ok");
    assert!(md.contains("| A | B |"), "全 th 首行未作表头: {md}");
    assert!(md.contains("| 1 | 2 |"), "数据行缺失: {md}");
}

/// role=presentation 布局表 → 展开为普通内容（非 GFM 表格）
#[test]
fn layout_table_expanded_not_rendered_as_table() {
    let html = r#"<table role="presentation"><tr><td>left</td><td>right</td></tr></table>"#;
    let md = default_converter()
        .convert(html)
        .expect("default converter ok");
    assert!(
        md.contains("left") && md.contains("right"),
        "布局表内容缺失: {md}"
    );
    assert!(
        !md.contains("| --- |"),
        "布局表不应产出 GFM 表格分隔行: {md}"
    );
}

/// 复杂表（colspan）→ 回退 HTML 段（GFM 无法表达的合并单元格）
#[test]
fn complex_table_falls_back_to_html_block() {
    let html =
        r#"<table><tr><td colspan="2">merged</td></tr><tr><td>a</td><td>b</td></tr></table>"#;
    let md = default_converter()
        .convert(html)
        .expect("default converter ok");
    assert!(md.contains("<table>"), "复杂表应回退 HTML 段: {md}");
    assert!(md.contains("colspan=\"2\""), "合并单元格属性应保留: {md}");
}

// ---- 2. Math 保真（MathML 净化不泄漏标签；alttext/annotation 走 TeX）----

#[test]
fn mathml_content_kept_without_raw_tags() {
    // 无 alttext/annotation 的裸 MathML：不得泄漏原始 <math> 标签到正文
    let html = r#"<p>Formula: <math><mi>x</mi><mo>=</mo><mn>1</mn></math> end</p>"#;
    let md = default_converter()
        .convert(html)
        .expect("default converter ok");
    assert!(!md.contains("<math>"), "裸 MathML 不得原样泄漏: {md}");
    assert!(md.contains("Formula:"), "正文文本应保留: {md}");
    assert!(md.contains("x=1"), "数学内容应净化为文本保留: {md}");

    // 带 alttext 的 MathML：math 保真路由 → $TeX$（旧 moli 会泄漏原始 MathML）
    let html = r#"<p>Euler: <math alttext="e^{i\pi}+1=0"><mrow><mi>e</mi></mrow></math></p>"#;
    let md = default_converter()
        .convert(html)
        .expect("default converter ok");
    assert!(md.contains("$e^{i\\pi}+1=0$"), "alttext 应转为 TeX: {md}");
    assert!(!md.contains("<math>"), "MathML 不得泄漏: {md}");
}

// ---- 3. TeX → $..$ ----

#[test]
fn tex_delimiters_normalized_to_dollar() {
    let html = r#"<p>Inline <span class="math">\(E=mc^2\)</span> and $$a+b$$</p>"#;
    let md = default_converter()
        .convert(html)
        .expect("default converter ok");
    assert!(md.contains("$E=mc^2$"), "\\(..\\) 应转为 $..$: {md}");
    assert!(md.contains("$$a+b$$"), "块级 $$..$$ 应保留: {md}");
}

// ---- 4. 表单默认值 ----

#[test]
fn form_default_values_emitted() {
    let html = r#"<form><input type="text" name="user" value="bob"><select name="pick"><option value="1" selected>One</option><option value="2">Two</option></select><textarea name="note">hello</textarea></form>"#;
    let md = default_converter()
        .convert(html)
        .expect("default converter ok");
    assert!(md.contains("bob"), "input 默认值缺失: {md}");
    assert!(md.contains("One"), "select 选中项文本缺失: {md}");
    assert!(md.contains("hello"), "textarea 内容缺失: {md}");
}

// ---- 链兜底活体验证 ----

/// htmd 对同类样本均可转换（链 fallback 成员可用性）
#[test]
fn htmd_handles_same_samples() {
    for html in [
        r#"<table><thead><tr><th>A</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>"#,
        r#"<p><math><mi>x</mi></math></p>"#,
        r#"<p>\(x\)</p>"#,
        r#"<form><input name="a" value="v"></form>"#,
    ] {
        assert!(htmd().convert(html).is_ok(), "htmd 转换失败: {html}");
    }
}
