// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! LLM 提取 token 窗口分块（F8）
//!
//! 长文超出模型上下文窗口时：按窗口预算切块 → tokio JoinSet 并行提问 →
//! 单次 merge prompt 综合（可关，退化为字段级启发式拼接）→ TokenUsage 全链累加。
//!
//! 预算估算口径（零新依赖，不用 tokenizer 精确计数）：
//! `budget_chars = (model_context_tokens − prompt_overhead_tokens − max_output_tokens)
//! × chars_per_token ÷ 2`（50% 余量覆盖估算偏差；CJK 场景建议把 chars_per_token 调低）。
//!
//! 切块边界三级回退：段落（`\n\n` / `\r\n\r\n`）→ 句子（`。！？.!?`）→ 按字符硬切；
//! 块间无重叠严格推进，拼接覆盖原文。
//!
//! 术语预检：schema 属性名拆词（驼峰/下划线/非字母数字切分，小写化），
//! 停用词过滤后全部不在原文中 → 确定性短路（省 token，规则 7）。

use serde_json::{json, Value};
use std::ops::Range;
use std::sync::Arc;

use crate::domain::services::llm::TokenUsage;

/// 分块提取的逐块上下文说明（让 LLM 对本块缺失字段输出 null）
const CHUNK_PART_NOTE: &str = "Note: this text is part {i} of {n} of a longer document. \
If the requested information is not present in this part, set the corresponding fields to null.";

/// merge prompt 指令前缀（复用 build_prompt 的 {{text}} 注入，schema 由模板注入）
const MERGE_INSTRUCTION: &str = "The following JSON array contains partial extraction results \
produced from consecutive parts of a longer document. Combine them into a single result that \
matches the requested schema: choose the best non-null value for each field, concatenate arrays \
for list fields, and output null for fields that are null in every part.\n\nPartial results:\n";

/// 内置停用词表：schema 通用属性名不参与术语预检（防误杀）
const TERM_STOPWORDS: &[&str] = &[
    "id",
    "name",
    "type",
    "value",
    "data",
    "time",
    "url",
    "title",
    "text",
    "key",
    "date",
    "count",
    "index",
    "description",
    "content",
    "items",
    "properties",
];

/// 分块配置（由 `[llm.chunking]` 七键 + enabled 构造）
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkingConfig {
    /// 总开关：false 时长文单 prompt 直发（现状，向后兼容）
    pub enabled: bool,
    /// 模型上下文窗口（token）
    pub model_context_tokens: u32,
    /// 单次回答预留输出 token
    pub max_output_tokens: u32,
    /// prompt 模板/边界标记等注入开销预留 token
    pub prompt_overhead_tokens: u32,
    /// token→字符保守换算（÷2 余量在 [`budget_chars`] 内固定）
    pub chars_per_token: f64,
    /// JoinSet 并行上限
    pub max_parallel: usize,
    /// 分块结果单次 merge prompt 综合（false 时逐块结果直接按字段拼接）
    pub merge_enabled: bool,
    /// 术语预检短路开关
    pub term_precheck_enabled: bool,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model_context_tokens: 8192,
            max_output_tokens: 2048,
            prompt_overhead_tokens: 1024,
            chars_per_token: 4.0,
            max_parallel: 4,
            merge_enabled: true,
            term_precheck_enabled: true,
        }
    }
}

impl ChunkingConfig {
    /// 从 `[llm.chunking]` 配置构造
    pub fn from_settings(settings: &crate::config::settings::LlmChunkingSettings) -> Self {
        Self {
            enabled: settings.enabled,
            model_context_tokens: settings.model_context_tokens,
            max_output_tokens: settings.max_output_tokens,
            prompt_overhead_tokens: settings.prompt_overhead_tokens,
            chars_per_token: settings.chars_per_token,
            max_parallel: settings.max_parallel,
            merge_enabled: settings.merge_enabled,
            term_precheck_enabled: settings.term_precheck_enabled,
        }
    }
}

/// 分块预算（字符）：(窗口 − 开销 − 输出) × chars_per_token ÷ 2，下限 1
pub fn budget_chars(cfg: &ChunkingConfig) -> usize {
    let avail = cfg
        .model_context_tokens
        .saturating_sub(cfg.prompt_overhead_tokens)
        .saturating_sub(cfg.max_output_tokens);
    let est = (avail as f64) * cfg.chars_per_token / 2.0;
    (est as usize).max(1)
}

/// 按三级边界把超预算文本切块（段落 → 句子 → 硬切）
///
/// 块间无重叠严格推进，拼接覆盖原文；每块字符数 ≤ 预算。
/// 文本未超预算时返回覆盖全文的单块。
pub fn plan_chunks(text: &str, cfg: &ChunkingConfig) -> Vec<Range<usize>> {
    let budget = budget_chars(cfg);
    if text.chars().count() <= budget {
        return vec![Range {
            start: 0,
            end: text.len(),
        }];
    }

    let mut chunks: Vec<Range<usize>> = Vec::new();
    let mut chunk_start: usize = 0;
    let mut chunk_chars: usize = 0;

    for seg in paragraph_segments(text) {
        let seg_chars = text[seg.clone()].chars().count();

        if seg_chars > budget {
            // 先冲刷当前累积，再对该段做句子/硬切细分（每子块独立成块）
            if chunk_chars > 0 {
                chunks.push(chunk_start..seg.start);
                chunk_chars = 0;
            }
            for sub in split_oversized_segment(&text[seg.clone()], budget) {
                chunks.push(seg.start + sub.start..seg.start + sub.end);
            }
            chunk_start = seg.end;
            continue;
        }

        if chunk_chars > 0 && chunk_chars + seg_chars > budget {
            chunks.push(chunk_start..seg.start);
            chunk_start = seg.start;
            chunk_chars = 0;
        }
        if chunk_chars == 0 {
            chunk_start = seg.start;
        }
        chunk_chars += seg_chars;
    }

    if chunk_start < text.len() {
        chunks.push(chunk_start..text.len());
    }
    chunks
}

/// 段落切分（tiling）：`\n\n` / `\r\n\r\n` 为界，分隔符归前段
fn paragraph_segments(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut segs: Vec<Range<usize>> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let sep_len = if bytes[i..].starts_with(b"\r\n\r\n") {
            4
        } else if bytes[i..].starts_with(b"\n\n") {
            2
        } else {
            0
        };
        if sep_len > 0 {
            let end = i + sep_len;
            segs.push(start..end);
            start = end;
            i = end;
        } else {
            i += 1;
        }
    }
    if start < text.len() {
        segs.push(start..text.len());
    }
    segs
}

/// 句子切分（tiling）：句末标点（含 UTF-8 字节数）归前句
fn sentence_segments(seg: &str) -> Vec<Range<usize>> {
    let mut segs: Vec<Range<usize>> = Vec::new();
    let mut start = 0usize;
    for (bi, c) in seg.char_indices() {
        if matches!(c, '。' | '！' | '？' | '.' | '!' | '?') {
            let end = bi + c.len_utf8();
            segs.push(start..end);
            start = end;
        }
    }
    if start < seg.len() {
        segs.push(start..seg.len());
    }
    segs
}

/// 超预算段落细分：句子累积，超预算句子按字符硬切；返回段内 tiling 子区间
fn split_oversized_segment(seg: &str, budget: usize) -> Vec<Range<usize>> {
    let mut pieces: Vec<Range<usize>> = Vec::new();
    let mut acc_start = 0usize;
    let mut acc_chars = 0usize;

    for s in sentence_segments(seg) {
        let s_chars = seg[s.clone()].chars().count();

        if s_chars > budget {
            if acc_chars > 0 {
                pieces.push(acc_start..s.start);
                acc_chars = 0;
            }
            // 硬切：每 budget 个字符一块（UTF-8 字符边界安全）
            let mut pos = s.start;
            let mut used = 0usize;
            for (bi, _) in seg[s.start..s.end].char_indices() {
                if used == budget {
                    pieces.push(pos..s.start + bi);
                    pos = s.start + bi;
                    used = 0;
                }
                used += 1;
            }
            if pos < s.end {
                pieces.push(pos..s.end);
            }
            acc_start = s.end;
            continue;
        }

        if acc_chars > 0 && acc_chars + s_chars > budget {
            pieces.push(acc_start..s.start);
            acc_start = s.start;
            acc_chars = 0;
        }
        if acc_chars == 0 {
            acc_start = s.start;
        }
        acc_chars += s_chars;
    }

    if acc_start < seg.len() && acc_chars > 0 {
        pieces.push(acc_start..seg.len());
    }
    pieces
}

/// 术语预检：schema 属性名拆词（停用词过滤后）全部不在文本中 → false
///
/// 无属性名 / 全停用词 → true（不误杀，跳过判定）。
///
/// 匹配按词元逐个做 ASCII 大小写不敏感滑窗查找（`split_terms` 产出的词元
/// 恒为 ASCII 小写），不构造整篇文本的小写副本。
pub fn precheck(schema: &Value, text: &str) -> bool {
    let mut terms = Vec::new();
    collect_schema_terms(schema, &mut terms);
    let meaningful: Vec<&String> = terms
        .iter()
        .filter(|t| !TERM_STOPWORDS.contains(&t.as_str()))
        .collect();
    if meaningful.is_empty() {
        return true;
    }
    meaningful
        .iter()
        .all(|t| contains_ascii_ci(text, t.as_str()))
}

/// ASCII 大小写不敏感子串查找（无分配；`needle` 须为 ASCII——`split_terms` 保证）
fn contains_ascii_ci(haystack: &str, needle: &str) -> bool {
    let n = needle.as_bytes();
    if n.is_empty() {
        return true;
    }
    let h = haystack.as_bytes();
    if n.len() > h.len() {
        return false;
    }
    h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// 递归收集 schema `properties` 的属性名
fn collect_schema_terms(schema: &Value, out: &mut Vec<String>) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::Object(props)) = map.get("properties") {
                for (name, prop_schema) in props {
                    out.extend(split_terms(name));
                    collect_schema_terms(prop_schema, out);
                }
            }
            if let Some(items) = map.get("items") {
                collect_schema_terms(items, out);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_schema_terms(v, out);
            }
        }
        _ => {}
    }
}

/// 属性名拆词：按非字母数字 + 驼峰边界切分，小写化
fn split_terms(name: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for seg in name.split(|c: char| !c.is_ascii_alphanumeric()) {
        if seg.is_empty() {
            continue;
        }
        let mut cur = String::new();
        for c in seg.chars() {
            if !cur.is_empty() && c.is_ascii_uppercase() {
                terms.push(cur.to_lowercase());
                cur = String::new();
            }
            cur.push(c);
        }
        if !cur.is_empty() {
            terms.push(cur.to_lowercase());
        }
    }
    terms
}

/// 字段级启发式 merge：对象逐字段取首个非 null（数组字段 concat），根数组直接 concat
pub fn merge_results(results: &[Value]) -> Value {
    if results.is_empty() {
        return json!({});
    }
    if results.iter().all(Value::is_array) {
        let mut merged = Vec::new();
        for r in results {
            if let Value::Array(items) = r {
                merged.extend(items.iter().cloned());
            }
        }
        return Value::Array(merged);
    }
    let mut merged = serde_json::Map::new();
    for r in results {
        if let Value::Object(map) = r {
            for (k, v) in map {
                match merged.get(k) {
                    Some(Value::Array(existing)) if v.is_array() => {
                        let mut concat = existing.clone();
                        if let Value::Array(items) = v {
                            concat.extend(items.iter().cloned());
                        }
                        merged.insert(k.clone(), Value::Array(concat));
                    }
                    Some(Value::Null) | None => {
                        merged.insert(k.clone(), v.clone());
                    }
                    Some(Value::Array(existing)) if v.is_null() => {
                        // 已聚合数组 vs null：保留聚合结果
                        merged.insert(k.clone(), Value::Array(existing.clone()));
                    }
                    _ => {
                        // 已有非 null 值：保留首个非 null
                    }
                }
            }
        }
    }
    Value::Object(merged)
}

/// 分块提取编排：术语预检 → 预算判定 → 切块并行提取 → merge 综合
///
/// 回调由调用方注入（LLMService 接 provider 路径；测试接 mock），使编排逻辑
/// 与具体 provider 解耦：
/// - `extract_one`：单次提取（切块后的文本 → 结构化结果）
/// - `merge_call`：merge prompt 调用（指令 + 块结果 JSON 数组 → 最终结果）
///
/// TokenUsage 累加 = Σ块 + merge 次调用。
/// 任一块提取失败 → 整体 Err（失败显性化，由上层 retry_policy 承接）。
pub async fn extract_chunked<E, FutE, M, FutM>(
    text: &str,
    schema: &Value,
    cfg: &ChunkingConfig,
    extract_one: E,
    merge_call: M,
) -> anyhow::Result<(Value, TokenUsage)>
where
    E: Fn(String) -> FutE + Clone + Send + 'static,
    FutE: std::future::Future<Output = anyhow::Result<(Value, TokenUsage)>> + Send,
    M: Fn(String) -> FutM + Clone + Send + 'static,
    FutM: std::future::Future<Output = anyhow::Result<(Value, TokenUsage)>> + Send,
{
    // 术语预检：确定性短路（省 token）
    if cfg.term_precheck_enabled && !precheck(schema, text) {
        log::warn!(
            "llm_chunking: term precheck failed — schema terms absent from text, short-circuit"
        );
        return Ok((json!({}), TokenUsage::default()));
    }

    // 未超预算：单发直走（现状语义）
    if text.chars().count() <= budget_chars(cfg) {
        return extract_one(text.to_string()).await;
    }

    let ranges = plan_chunks(text, cfg);
    let total = ranges.len();
    let chunk_texts: Vec<String> = ranges
        .iter()
        .enumerate()
        .map(|(i, r)| {
            format!(
                "{note}\n\n{chunk}",
                note = CHUNK_PART_NOTE
                    .replace("{i}", &(i + 1).to_string())
                    .replace("{n}", &total.to_string()),
                chunk = &text[r.clone()],
            )
        })
        .collect();

    // JoinSet + 信号量限流：并发峰值 ≤ max_parallel
    let semaphore = Arc::new(tokio::sync::Semaphore::new(cfg.max_parallel.max(1)));
    let mut join_set: tokio::task::JoinSet<(usize, anyhow::Result<(Value, TokenUsage)>)> =
        tokio::task::JoinSet::new();
    for (idx, chunk_text) in chunk_texts.into_iter().enumerate() {
        let permit_sem = semaphore.clone();
        let extract_one = extract_one.clone();
        join_set.spawn(async move {
            let _permit = permit_sem.acquire_owned().await;
            (idx, extract_one(chunk_text).await)
        });
    }

    let mut per_chunk: Vec<Option<(Value, TokenUsage)>> = (0..total).map(|_| None).collect();
    while let Some(joined) = join_set.join_next().await {
        let (idx, result) = joined.map_err(|e| anyhow::anyhow!("chunk task panicked: {e}"))?;
        let (value, usage) = result?;
        per_chunk[idx] = Some((value, usage));
    }

    let mut total_usage = TokenUsage::default();
    let mut chunk_values: Vec<Value> = Vec::with_capacity(total);
    for (value, usage) in per_chunk.into_iter().flatten() {
        total_usage.prompt_tokens += usage.prompt_tokens;
        total_usage.completion_tokens += usage.completion_tokens;
        total_usage.total_tokens += usage.total_tokens;
        chunk_values.push(value);
    }

    // merge：单次 merge prompt 综合；失败或关闭时确定性退化启发式拼接
    if cfg.merge_enabled && chunk_values.len() > 1 {
        let payload = format!(
            "{instruction}{results}",
            instruction = MERGE_INSTRUCTION,
            results = serde_json::to_string(&chunk_values)?,
        );
        match merge_call(payload).await {
            Ok((merged, usage)) => {
                total_usage.prompt_tokens += usage.prompt_tokens;
                total_usage.completion_tokens += usage.completion_tokens;
                total_usage.total_tokens += usage.total_tokens;
                return Ok((merged, total_usage));
            }
            Err(e) => {
                log::warn!(
                    "llm_chunking: merge call failed ({e}); falling back to heuristic merge"
                );
            }
        }
    }

    Ok((merge_results(&chunk_values), total_usage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Mutex;

    // ===== 预算算术 =====

    /// 默认配置代入：(8192 − 1024 − 2048) × 4 ÷ 2 = 10240 字符
    #[test]
    fn budget_chars_default_config_arithmetic() {
        let cfg = ChunkingConfig::default();
        assert_eq!(budget_chars(&cfg), 10240);
    }

    /// 预算下限 1（退化配置不死循环/不 panic）
    #[test]
    fn budget_chars_floors_at_one() {
        let cfg = ChunkingConfig {
            model_context_tokens: 100,
            prompt_overhead_tokens: 200,
            max_output_tokens: 300,
            ..ChunkingConfig::default()
        };
        assert_eq!(budget_chars(&cfg), 1);
    }

    // ===== plan_chunks =====

    /// 未超预算文本返回覆盖全文的单块
    #[test]
    fn plan_chunks_single_block_when_under_budget() {
        let cfg = cfg_with_budget(100);
        let text = "short text".to_string();
        let chunks = plan_chunks(&text, &cfg);
        assert_eq!(chunks, vec![0..text.len()]);
    }

    /// 超预算文本：块数 = ceil(总字符/预算)，每块 ≤ 预算，拼接覆盖原文（无缝无叠）
    #[test]
    fn plan_chunks_chunk_count_matches_ceil_and_tiles_text() {
        // 40 段 × (10 字符 + "\n\n")，末段无分隔符 → 478 字符；预算 100 → ceil = 5
        let mut text = String::new();
        for i in 0..40 {
            text.push_str(&format!("para-{:02}-x\n\n", i));
        }
        let text = text.trim_end_matches('\n').to_string();
        let cfg = cfg_with_budget(100);
        let chunks = plan_chunks(&text, &cfg);
        assert_eq!(
            chunks.len(),
            text.chars().count().div_ceil(100),
            "块数应为 ceil(len/budget)"
        );
        assert_tiles(text.as_str(), &chunks);
    }

    /// 段落边界优先：块切在 `\n\n` 处
    #[test]
    fn plan_chunks_prefers_paragraph_boundaries() {
        let cfg = cfg_with_budget(30);
        let text = "aa bb cc dd\n\nee ff gg hh\n\nii jj kk ll mm nn";
        let chunks = plan_chunks(text, &cfg);
        assert_tiles(text, &chunks);
        // 每块末尾应是段落分隔符或文本结尾
        for (i, r) in chunks.iter().enumerate() {
            let seg = &text[r.clone()];
            let is_last = i == chunks.len() - 1;
            assert!(
                is_last || seg.ends_with("\n\n"),
                "块应切在段落边界: {seg:?}"
            );
        }
    }

    /// CRLF 文本（Windows 行尾）段落边界同样生效
    #[test]
    fn plan_chunks_handles_crlf_paragraphs() {
        let cfg = cfg_with_budget(20);
        let text = "aa bb cc\r\n\r\ndd ee ff\r\n\r\ngg hh ii";
        let chunks = plan_chunks(text, &cfg);
        assert_tiles(text, &chunks);
    }

    /// 无段落时切句子边界
    #[test]
    fn plan_chunks_falls_back_to_sentence_boundaries() {
        let cfg = cfg_with_budget(25);
        let text = "First sentence here. Second sentence follows. Third one is longer!";
        let chunks = plan_chunks(text, &cfg);
        assert!(chunks.len() > 1, "超预算应多块");
        assert_tiles(text, &chunks);
        // 块应尽量切在句末标点后
        let last = chunks.last().unwrap();
        assert!(text[last.clone()].ends_with('!'));
    }

    /// 超长无标点句硬切：每块 ≤ 预算且拼接覆盖原文
    #[test]
    fn plan_chunks_hard_cuts_oversized_sentence() {
        let cfg = cfg_with_budget(10);
        let text = "abcdefghij".to_string();
        let text = format!("{text}{text}{text}"); // 30 字符无段落无句末标点
        let chunks = plan_chunks(&text, &cfg);
        assert_eq!(chunks.len(), 3);
        assert_tiles(&text, &chunks);
        for r in &chunks {
            assert!(text[r.clone()].chars().count() <= 10);
        }
    }

    /// UTF-8 字符边界安全：CJK 多字节字符硬切不 panic、不撕裂
    #[test]
    fn plan_chunks_hard_cut_respects_utf8_boundaries() {
        let cfg = cfg_with_budget(3);
        let text = "一二三四五六七八九十".to_string(); // 10 个 CJK 字符
        let chunks = plan_chunks(&text, &cfg);
        assert_tiles(&text, &chunks);
        for r in &chunks {
            assert!(text[r.clone()].chars().count() <= 3);
        }
    }

    /// 混合场景：段落内含超预算超长句
    #[test]
    fn plan_chunks_mixed_paragraph_with_oversized_sentence() {
        let cfg = cfg_with_budget(8);
        let text = format!("short.\n\n{}", "x".repeat(30));
        let chunks = plan_chunks(&text, &cfg);
        assert_tiles(&text, &chunks);
        for r in &chunks {
            assert!(text[r.clone()].chars().count() <= 8, "块 {:?} 超预算", r);
        }
    }

    /// ChunkingConfig::default() 是禁用态但保留默认预算参数（与 default.toml 对齐）
    #[test]
    fn default_config_is_disabled_with_spec_defaults() {
        let cfg = ChunkingConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.model_context_tokens, 8192);
        assert_eq!(cfg.max_output_tokens, 2048);
        assert_eq!(cfg.prompt_overhead_tokens, 1024);
        assert!((cfg.chars_per_token - 4.0).abs() < f64::EPSILON);
        assert_eq!(cfg.max_parallel, 4);
        assert!(cfg.merge_enabled);
        assert!(cfg.term_precheck_enabled);
    }

    // ===== 术语预检 =====

    /// schema 字段 product_price 在含 "product price" 文本中通过
    #[test]
    fn precheck_passes_when_terms_present_in_text() {
        let schema = json!({
            "type": "object",
            "properties": { "product_price": { "type": "number" } }
        });
        assert!(precheck(&schema, "The product price is 42 dollars."));
    }

    /// 驼峰命名等价拆词
    #[test]
    fn precheck_splits_camel_case_terms() {
        let schema = json!({
            "type": "object",
            "properties": { "productPrice": { "type": "number" } }
        });
        assert!(precheck(&schema, "the product price"));
    }

    /// 字段 qwzx 不在文本中 → false
    #[test]
    fn precheck_fails_when_terms_absent_from_text() {
        let schema = json!({
            "type": "object",
            "properties": { "qwzx": { "type": "string" } }
        });
        assert!(!precheck(&schema, "nothing relevant here at all."));
    }

    /// 嵌套 schema 属性名递归收集
    #[test]
    fn precheck_collects_nested_property_names() {
        let schema = json!({
            "type": "object",
            "properties": {
                "author": {
                    "type": "object",
                    "properties": { "biography": { "type": "string" } }
                }
            }
        });
        assert!(precheck(&schema, "author biography mentioned"));
        assert!(!precheck(&schema, "neither word appears here"));
    }

    /// 停用词：schema 仅含 {"id","name"} 时不误杀（全停用词跳过判定）
    #[test]
    fn precheck_all_stopwords_skips_verdict() {
        let schema = json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" }
            }
        });
        assert!(precheck(&schema, "completely unrelated text"));
    }

    /// 无属性名的 schema（如 {"type":"array"}）不误杀
    #[test]
    fn precheck_schema_without_properties_passes() {
        let schema = json!({ "type": "array", "description": "list of items" });
        assert!(precheck(&schema, "any text"));
    }

    // ===== 启发式 merge =====

    /// 对象：逐字段取首个非 null
    #[test]
    fn merge_results_takes_first_non_null_per_field() {
        let results = vec![
            json!({"title": "from chunk 1", "x": null}),
            json!({"title": "from chunk 2"}),
            json!({"title": null, "y": 2}),
        ];
        let merged = merge_results(&results);
        assert_eq!(merged["title"], "from chunk 1");
        assert_eq!(merged["x"], Value::Null);
        assert_eq!(merged["y"], 2);
    }

    /// 数组字段 concat
    #[test]
    fn merge_results_concatenates_array_fields() {
        let results = vec![json!({"items": [1, 2]}), json!({"items": [3]})];
        let merged = merge_results(&results);
        assert_eq!(merged["items"], json!([1, 2, 3]));
    }

    /// 根数组 schema：直接 concat
    #[test]
    fn merge_results_concatenates_root_arrays() {
        let results = vec![json!([{"a": 1}]), json!([{"b": 2}])];
        let merged = merge_results(&results);
        assert_eq!(merged, json!([{"a": 1}, {"b": 2}]));
    }

    /// 空结果退化空对象
    #[test]
    fn merge_results_empty_input_yields_empty_object() {
        assert_eq!(merge_results(&[]), json!({}));
    }

    // ===== extract_chunked 编排 =====

    /// 未超预算：单发直走，回调收到原文（不切块、不加块注记）
    #[tokio::test]
    async fn extract_chunked_single_shot_when_under_budget() {
        let cfg = cfg_with_budget(1000);
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_c = calls.clone();
        let (value, usage) = extract_chunked(
            "plain short text about products",
            &json!({"type": "object", "properties": {"product": {"type": "string"}}}),
            &cfg,
            move |text: String| {
                let calls = calls_c.clone();
                async move {
                    calls.lock().unwrap().push(text);
                    Ok((json!({"product": "p"}), usage_of(10, 5)))
                }
            },
            move |_payload: String| async { anyhow::bail!("merge should not be called") },
        )
        .await
        .unwrap();
        assert_eq!(value, json!({"product": "p"}));
        assert_eq!(usage.total_tokens, 15);
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "plain short text about products");
    }

    /// 术语预检失败：确定性短路，不调用 provider，返回空对象零用量
    #[tokio::test]
    async fn extract_chunked_precheck_short_circuits() {
        let cfg = cfg_with_budget(1000);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_c = calls.clone();
        let (value, usage) = extract_chunked(
            "nothing relevant here",
            &json!({"type": "object", "properties": {"qwzx": {"type": "string"}}}),
            &cfg,
            move |_text: String| {
                let calls = calls_c.clone();
                async move {
                    calls.fetch_add(1, AtomicOrdering::SeqCst);
                    Ok((json!({}), usage_of(1, 1)))
                }
            },
            move |_payload: String| async { anyhow::bail!("merge should not be called") },
        )
        .await
        .unwrap();
        assert_eq!(value, json!({}));
        assert_eq!(usage.total_tokens, 0);
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 0, "provider 不应被调用");
    }

    /// term_precheck_enabled=false 时不短路
    #[tokio::test]
    async fn extract_chunked_precheck_disabled_proceeds() {
        let cfg = ChunkingConfig {
            term_precheck_enabled: false,
            ..cfg_with_budget(1000)
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_c = calls.clone();
        let _ = extract_chunked(
            "nothing relevant here",
            &json!({"type": "object", "properties": {"qwzx": {"type": "string"}}}),
            &cfg,
            move |_text: String| {
                let calls = calls_c.clone();
                async move {
                    calls.fetch_add(1, AtomicOrdering::SeqCst);
                    Ok((json!({"qwzx": "found"}), usage_of(1, 1)))
                }
            },
            move |_payload: String| async { anyhow::bail!("merge should not be called") },
        )
        .await
        .unwrap();
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
    }

    /// 分块并行：每块都带 part 注记，并发峰值 ≤ max_parallel（且确实并行）
    #[tokio::test]
    async fn extract_chunked_runs_parallel_within_max_parallel() {
        let cfg = ChunkingConfig {
            max_parallel: 2,
            ..cfg_with_budget(20)
        };
        // 4 段 × (17 字符 + "\n\n")，预算 20 → 每段一段一块，共 4 块
        let text = "aaaa bbbb cccc dd\neeee ffff gggg hh\niiii jjjj kkkk ll\nmmmm nnnn oooo pp"
            .to_string();
        // schema 用停用词字段，绕过术语预检（预检行为由专属测试覆盖）
        let schema = json!({"type": "object", "properties": {"text": {"type": "string"}}});

        let calls = Arc::new(AtomicUsize::new(0));
        let inflight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let texts = Arc::new(Mutex::new(Vec::<String>::new()));

        let (calls_c, inflight_c, peak_c, texts_c) =
            (calls.clone(), inflight.clone(), peak.clone(), texts.clone());
        let (value, usage) = extract_chunked(
            &text,
            &schema,
            &cfg,
            move |chunk: String| {
                let (calls, inflight, peak, texts) = (
                    calls_c.clone(),
                    inflight_c.clone(),
                    peak_c.clone(),
                    texts_c.clone(),
                );
                async move {
                    calls.fetch_add(1, AtomicOrdering::SeqCst);
                    let now = inflight.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                    peak.fetch_max(now, AtomicOrdering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    inflight.fetch_sub(1, AtomicOrdering::SeqCst);
                    texts.lock().unwrap().push(chunk);
                    Ok((json!({"text": "p"}), usage_of(7, 3)))
                }
            },
            move |_payload: String| async { anyhow::bail!("merge should not be called") },
        )
        .await
        .unwrap();

        assert_eq!(calls.load(AtomicOrdering::SeqCst), 4, "4 块各提取一次");
        assert!(
            peak.load(AtomicOrdering::SeqCst) <= 2,
            "并发峰值必须 ≤ max_parallel"
        );
        assert!(
            peak.load(AtomicOrdering::SeqCst) >= 2,
            "JoinSet 应真实并行（峰值 > 1）"
        );
        // 逐块注记存在
        let texts = texts.lock().unwrap();
        for (i, t) in texts.iter().enumerate() {
            assert!(t.contains("part"), "块 {i} 应带 part 注记");
        }
        // merge 回调失败 → 启发式兜底；4 块结果一致 → 取首个非 null
        assert_eq!(value, json!({"text": "p"}));
        // TokenUsage 累加 = Σ块（merge 失败退化，无 merge 用量）
        assert_eq!(usage.prompt_tokens, 28);
        assert_eq!(usage.completion_tokens, 12);
        assert_eq!(usage.total_tokens, 40);
    }

    /// merge：三块各出 {"title": ...} → merge prompt 收到 JSON 数组，返回合并结果；
    /// TokenUsage 累加 = Σ块 + merge 次调用
    #[tokio::test]
    async fn extract_chunked_merge_prompt_combines_chunk_results() {
        let cfg = ChunkingConfig {
            merge_enabled: true,
            ..cfg_with_budget(20)
        };
        // 3 段均 ≤ 预算 20（含分隔符）→ 3 块
        let text = "alpha beta gamma\n\nzeta eta theta\n\niota kappa lamb".to_string();
        let merge_payloads = Arc::new(Mutex::new(Vec::<String>::new()));
        let merge_payloads_c = merge_payloads.clone();

        let (value, usage) = extract_chunked(
            &text,
            &json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            &cfg,
            move |chunk: String| {
                let title = if chunk.contains("zeta") {
                    "title-zeta"
                } else {
                    "title-other"
                };
                async move { Ok((json!({ "title": title }), usage_of(10, 2))) }
            },
            move |payload: String| {
                let payloads = merge_payloads_c.clone();
                async move {
                    payloads.lock().unwrap().push(payload);
                    Ok((json!({"title": "merged-best"}), usage_of(30, 5)))
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(value, json!({"title": "merged-best"}));
        let payloads = merge_payloads.lock().unwrap();
        assert_eq!(payloads.len(), 1, "merge 只调一次");
        let payload = &payloads[0];
        assert!(payload.contains("Combine them into a single result"));
        assert!(payload.contains("\"title\""), "应含块结果 JSON 数组字段");
        assert!(
            payload.contains("title-zeta") && payload.contains("title-other"),
            "应含各块结果: {payload}"
        );
        // Σ块 = 3 × (10+2=12) = 36 + merge (30+5=35) = 71
        assert_eq!(usage.prompt_tokens, 30 + 30);
        assert_eq!(usage.completion_tokens, 6 + 5);
        assert_eq!(usage.total_tokens, 36 + 35);
    }

    /// merge_enabled=false：不调 merge prompt，启发式拼接（非 null 首个）
    #[tokio::test]
    async fn extract_chunked_merge_disabled_uses_heuristic_merge() {
        let cfg = ChunkingConfig {
            merge_enabled: false,
            ..cfg_with_budget(20)
        };
        let text = "alpha beta gamma\n\nzeta eta theta\n\niota kappa lamb".to_string();
        let (value, usage) = extract_chunked(
            &text,
            &json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            &cfg,
            move |chunk: String| {
                let title = if chunk.contains("zeta") {
                    "winner-zeta"
                } else {
                    ""
                };
                async move {
                    Ok((
                        if title.is_empty() {
                            json!({"title": null})
                        } else {
                            json!({"title": title})
                        },
                        usage_of(10, 2),
                    ))
                }
            },
            move |_payload: String| async { anyhow::bail!("merge must not be called") },
        )
        .await
        .unwrap();
        assert_eq!(value, json!({"title": "winner-zeta"}), "取首个非 null");
        assert_eq!(usage.total_tokens, 36, "仅 Σ块用量");
    }

    /// merge prompt 失败 → 显性 warn + 启发式兜底（不整体失败）
    #[tokio::test]
    async fn extract_chunked_merge_failure_falls_back_to_heuristic() {
        let cfg = cfg_with_budget(20);
        let text = "alpha beta gamma\n\nzeta eta theta\n\niota kappa lamb".to_string();
        let (value, usage) = extract_chunked(
            &text,
            &json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            &cfg,
            move |_chunk: String| async { Ok((json!({"title": "chunk-title"}), usage_of(10, 2))) },
            move |_payload: String| async { anyhow::bail!("merge provider down") },
        )
        .await
        .unwrap();
        assert_eq!(
            value,
            json!({"title": "chunk-title"}),
            "启发式兜底取首个非 null"
        );
        assert_eq!(usage.total_tokens, 36);
    }

    /// 任一块提取失败 → 整体 Err（失败显性化，不静默丢块）
    #[tokio::test]
    async fn extract_chunked_chunk_failure_propagates_error() {
        let cfg = cfg_with_budget(20);
        let text =
            "alpha beta gamma delta\n\nepsilon zeta eta theta\n\niota kappa lambda mu".to_string();
        let result = extract_chunked(
            &text,
            &json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            &cfg,
            move |chunk: String| async move {
                if chunk.contains("epsilon") {
                    anyhow::bail!("provider 429")
                }
                Ok((json!({"title": "ok"}), usage_of(1, 1)))
            },
            move |_payload: String| async { anyhow::bail!("merge must not be called") },
        )
        .await;
        let err = result.expect_err("块失败必须整体 Err");
        assert!(err.to_string().contains("429"), "错误信息保留: {err}");
    }

    /// TokenUsage 在无块产出时为零值
    #[tokio::test]
    async fn extract_chunked_empty_usage_default() {
        let cfg = cfg_with_budget(1000);
        let (_, usage) = extract_chunked(
            "tiny text",
            &json!({"type": "object", "properties": {"product": {"type": "string"}}}),
            &cfg,
            move |_text: String| async { Ok((json!({}), TokenUsage::default())) },
            move |_payload: String| async { anyhow::bail!("merge must not be called") },
        )
        .await
        .unwrap();
        assert_eq!(usage, TokenUsage::default());
    }

    // ===== helpers =====

    fn usage_of(prompt: u32, completion: u32) -> TokenUsage {
        TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
        }
    }

    /// 构造预算 = n 字符的配置：(8192 − 1024 − out) × 4 ÷ 2 = n → out = 8192 − 1024 − n/2
    fn cfg_with_budget(n: usize) -> ChunkingConfig {
        ChunkingConfig {
            enabled: true,
            max_output_tokens: (8192 - 1024 - (n / 2)) as u32,
            ..ChunkingConfig::default()
        }
    }

    /// 断言块区间 tiling：无缝无叠、按序拼接覆盖原文
    fn assert_tiles(text: &str, chunks: &[Range<usize>]) {
        assert!(!chunks.is_empty(), "至少一块");
        let mut cursor = 0usize;
        for r in chunks {
            assert_eq!(r.start, cursor, "块无缝衔接");
            assert!(r.end >= r.start);
            cursor = r.end;
        }
        assert_eq!(cursor, text.len(), "块拼接覆盖全文");
        let joined: String = chunks.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(joined, text);
    }
}
