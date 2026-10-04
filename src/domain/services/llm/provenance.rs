// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! LLM 提取结果字符级溯源对齐（F9；langextract 三层对齐的零新依赖实现）
//!
//! LLM 提取出的字段值（excerpt）与原文（source）做 token 级对齐，还原该值在
//! 原文中的字符区间（char 偏移，非字节——CJK 安全），供 RAG 入库链持久化：
//!
//! 1. **第一层**：excerpt 词元在 source 词元上做单调不回退的贪心最早匹配
//!    （含覆盖率上界提前剪枝）；
//! 2. **第二层**：第一层覆盖率不足门限时，在锚定滑窗（excerpt 字符长 × 4）内
//!    退化为最紧 LCS 区间；
//! 3. **双门**：coverage（匹配词元数 / excerpt 词元数）≥ `min_coverage`
//!    且 density（匹配字符数 / 区间字符数）≥ 1/3，全败则该字段置 null
//!    由调用方过滤。
//!
//! 词元化用 `regex \w+`（Unicode 词元，CJK 连续串整段成词）——词元边界精度
//! 差异由双门兜底，不引入 tokenizer 依赖（蓝图裁定）。
//!
//! 复杂度护栏：贪心 O(n+m)；LCS 仅在窗口词元数与 excerpt 词元数均不超过
//! [`MAX_LCS_TOKENS`] 时运行（O(n·m) 在亿级比较会失控），超限只信第一层。

use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

/// 对齐词元正则：`\w+`（Unicode 词元；CJK 连续串整段成词）
static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+").expect("valid regex"));

/// 覆盖率门默认下限（langextract 同款；可被 `provenance_min_coverage` 配置覆盖）
pub const DEFAULT_MIN_COVERAGE: f64 = 0.75;

/// 字符密度门：匹配字符数 / 区间字符数 ≥ 1/3（langextract 同源常量，代码内固定）
pub const DENSITY_GATE: f64 = 1.0 / 3.0;

/// 第二层 LCS 的词元数上限（窗口与 excerpt 各自封顶）：超出则跳过 DP 只信第一层，
/// 防止超大字段值 × 超长原文的 O(n·m) 放大
const MAX_LCS_TOKENS: usize = 1024;

/// 滑窗宽度系数：窗口字符数 = excerpt 字符长 × 4（蓝图口径）
const WINDOW_FACTOR: usize = 4;

/// 溯源对齐配置（由 `[llm]` 的 `provenance_*` 两键构造）
#[derive(Debug, Clone, PartialEq)]
pub struct ProvenanceConfig {
    /// 总开关：false 时 `extract_data_with_provenance` 短路返回 None（零开销）
    pub enabled: bool,
    /// 覆盖率门下限（字符密度门 1/3 为代码内常量 [`DENSITY_GATE`]）
    pub min_coverage: f64,
}

impl Default for ProvenanceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_coverage: DEFAULT_MIN_COVERAGE,
        }
    }
}

impl ProvenanceConfig {
    /// 从 `[llm]` 配置构造
    pub fn from_settings(settings: &crate::config::settings::LLMSettings) -> Self {
        Self {
            enabled: settings.provenance_enabled,
            min_coverage: settings.provenance_min_coverage,
        }
    }
}

/// 字符区间 `[start, end)`（char 偏移，非字节——CJK 安全）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharInterval {
    /// 起始 char 偏移（含）
    pub start: usize,
    /// 结束 char 偏移（不含）
    pub end: usize,
}

/// 一次成功对齐：区间 + 覆盖率（provenance 条目需要 coverage 数值）
#[derive(Debug, Clone, PartialEq)]
pub struct AlignMatch {
    /// 原文字符区间
    pub interval: CharInterval,
    /// 匹配词元数 / excerpt 词元数
    pub coverage: f64,
}

/// 词元（内部表示）：小写化用于匹配；`len` 取原文词元的 char 长，
/// 避免个别 Unicode 字符小写化后长度漂移污染偏移
#[derive(Debug, Clone)]
struct Token {
    lower: String,
    /// 原文词元 char 起始偏移
    start: usize,
    /// 原文词元 char 长度
    len: usize,
}

/// 词元化：`\w+` 小写化 + char 偏移（公开给消费方/测试诊断用）
pub fn tokenize(s: &str) -> Vec<(String, usize)> {
    tokenize_tokens(s)
        .into_iter()
        .map(|t| (t.lower, t.start))
        .collect()
}

/// 内部词元化：保留原文 char 长度
fn tokenize_tokens(s: &str) -> Vec<Token> {
    let mut char_of_byte: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for (ci, (bi, _)) in s.char_indices().enumerate() {
        char_of_byte.insert(bi, ci);
    }
    // 结束哨兵：末尾 char 偏移（词元恰好在文末时计算 char 长度用）
    char_of_byte.insert(s.len(), s.chars().count());

    WORD_RE
        .find_iter(s)
        .map(|m| {
            let start = char_of_byte.get(&m.start()).copied().unwrap_or(0);
            let end = char_of_byte.get(&m.end()).copied().unwrap_or(start);
            Token {
                lower: m.as_str().to_lowercase(),
                start,
                len: end.saturating_sub(start),
            }
        })
        .collect()
}

/// 把原文（source）与提取摘录（excerpt）对齐，返回命中区间与覆盖率。
///
/// 双门任一不过、excerpt 为空、原文为空或 excerpt 首词不在原文中 → `None`
/// （不硬凑区间，幻觉摘录必须显性失败）。
pub fn align(source: &str, excerpt: &str, min_coverage: f64) -> Option<AlignMatch> {
    if excerpt.is_empty() || source.is_empty() {
        return None;
    }
    align_tokens(
        &tokenize_tokens(source),
        &tokenize_tokens(excerpt),
        source,
        excerpt,
        min_coverage,
    )
}

/// [`align`] 的预分词形式：同一 source 对齐多个字段时复用其词元序列，
/// 避免 provenance_map 逐字段重分词整篇原文（正则扫描 + char 偏移表）
fn align_tokens(
    src: &[Token],
    ex: &[Token],
    source: &str,
    excerpt: &str,
    min_coverage: f64,
) -> Option<AlignMatch> {
    if ex.is_empty() || src.is_empty() {
        return None;
    }
    let m = ex.len();

    // 覆盖率门换算为所需匹配词元数（钳制到 [1, m]，容忍越界配置）
    let need = ((min_coverage * m as f64).ceil() as usize).clamp(1, m);

    // 第一层：贪心最早单调匹配（含覆盖率上界提前剪枝）
    let greedy = greedy_match(src, ex, m - need);
    if let Some(res) = greedy_resolved(src, ex, &greedy, need, m) {
        return Some(res);
    }

    // 第二层：滑窗内最紧 LCS（窗口词元数或 excerpt 词元数超限则放弃）
    let window = dp_window(src, ex, &greedy.matches, excerpt, source)?;
    let matched = lcs_match(&src[window.clone()], ex);
    if matched.len() < need {
        return None;
    }
    let src_idx: Vec<usize> = matched.iter().map(|&j| window.start + j).collect();
    gates(src, ex, &src_idx, need, m)
}

/// 贪心最早单调匹配：excerpt 词元依次在 source 未消费前缀中找首个相等词元。
///
/// unmatched 超过 `allowed_miss`（覆盖率上界不可达）即提前剪枝（`pruned = true`）。
fn greedy_match(src: &[Token], ex: &[Token], allowed_miss: usize) -> GreedyResult {
    let mut matches: Vec<usize> = Vec::new();
    let mut j = 0usize;
    let mut unmatched = 0usize;
    for t in ex {
        let mut found = None;
        while j < src.len() {
            if src[j].lower == t.lower {
                found = Some(j);
                break;
            }
            j += 1;
        }
        match found {
            Some(k) => {
                matches.push(k);
                j = k + 1;
            }
            None => {
                unmatched += 1;
                if unmatched > allowed_miss {
                    return GreedyResult {
                        matches,
                        pruned: true,
                    };
                }
            }
        }
    }
    GreedyResult {
        matches,
        pruned: false,
    }
}

struct GreedyResult {
    /// 按 excerpt 顺序记录的 source 词元下标
    matches: Vec<usize>,
    /// 提前剪枝（覆盖率上界不可达）
    pruned: bool,
}

/// 第一层结果判定：完成且过双门 → 命中；否则交由第二层
fn greedy_resolved(
    src: &[Token],
    ex: &[Token],
    greedy: &GreedyResult,
    need: usize,
    m: usize,
) -> Option<AlignMatch> {
    if greedy.pruned || greedy.matches.len() < need {
        return None;
    }
    gates(src, ex, &greedy.matches, need, m)
}

/// 双门判定：coverage ≥ 门限（由 matched ≥ need 保证）且 density ≥ 1/3
fn gates(
    src: &[Token],
    ex: &[Token],
    matched_src_idx: &[usize],
    need: usize,
    m: usize,
) -> Option<AlignMatch> {
    if matched_src_idx.len() < need {
        return None;
    }
    let first = &src[matched_src_idx[0]];
    let last = &src[*matched_src_idx.last()?];
    let interval_chars = (last.start + last.len).saturating_sub(first.start);
    if interval_chars == 0 {
        return None;
    }
    let matched_chars: usize = matched_src_idx
        .iter()
        .enumerate()
        .map(|(i, _src_idx)| ex[i].len)
        .sum();
    let density = matched_chars as f64 / interval_chars as f64;
    if density < DENSITY_GATE {
        return None;
    }
    Some(AlignMatch {
        interval: CharInterval {
            start: first.start,
            end: last.start + last.len,
        },
        coverage: matched_src_idx.len() as f64 / m as f64,
    })
}

/// 第二层滑窗：锚定贪心首个命中（无命中锚 excerpt 首词的首个出现），
/// 覆盖贪心跨度并向两侧外扩 excerpt 字符长；词元数超 [`MAX_LCS_TOKENS`] → None
fn dp_window(
    src: &[Token],
    ex: &[Token],
    greedy_matches: &[usize],
    excerpt: &str,
    source: &str,
) -> Option<std::ops::Range<usize>> {
    if ex.len() > MAX_LCS_TOKENS {
        return None;
    }
    let excerpt_chars = excerpt.chars().count().max(1);
    let anchor = greedy_matches
        .first()
        .map(|&i| src[i].start)
        // 贪心零命中（如 excerpt 首词被 LLM 改写）：锚定任一 excerpt 词的
        // 最早出现位置（取 min 保证确定性）；全都不在原文 → 幻觉，None
        .or_else(|| {
            ex.iter()
                .filter_map(|t| src.iter().find(|s| s.lower == t.lower).map(|s| s.start))
                .min()
        })?;
    let lo = anchor.saturating_sub(excerpt_chars);
    let hi = greedy_matches
        .last()
        .map(|&i| src[i].start + src[i].len + excerpt_chars)
        .unwrap_or(anchor + excerpt_chars * WINDOW_FACTOR)
        .min(source.chars().count());
    let start = src.partition_point(|t| t.start + t.len <= lo);
    let end = src.partition_point(|t| t.start < hi);
    if end <= start || end - start > MAX_LCS_TOKENS {
        return None;
    }
    Some(start..end)
}

/// 最紧 LCS 单调匹配（经典 DP + 确定性回溯）：返回窗口内 source 词元下标
/// （按 excerpt 顺序）
fn lcs_match(src: &[Token], ex: &[Token]) -> Vec<usize> {
    let n = src.len();
    let m = ex.len();
    if n == 0 || m == 0 {
        return Vec::new();
    }
    let w = m + 1;
    let mut dp = vec![0u32; (n + 1) * w];
    for i in 1..=n {
        for j in 1..=m {
            dp[i * w + j] = if src[i - 1].lower == ex[j - 1].lower {
                dp[(i - 1) * w + (j - 1)] + 1
            } else {
                dp[(i - 1) * w + j].max(dp[i * w + (j - 1)])
            };
        }
    }
    let mut res = Vec::new();
    let mut i = n;
    let mut j = m;
    while i > 0 && j > 0 {
        if src[i - 1].lower == ex[j - 1].lower {
            res.push(i - 1);
            i -= 1;
            j -= 1;
        } else if dp[(i - 1) * w + j] >= dp[i * w + (j - 1)] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    res.reverse();
    res
}

/// 递归遍历提取结果，为字符串叶子产出 `{field_path: {start, end, coverage}}`
/// 溯源映射；对齐失败的字段值为 null（调用方过滤）。
///
/// 路径记法：对象 `parent.key`、数组 `parent[i]`、根数组元素 `[i]`、
/// 根标量字符串 `$`。数字/布尔/null 叶子无可对齐文本，不产条目；
/// `_provenance` 键跳过（防二次处理）。
pub fn provenance_map(source: &str, value: &Value, min_coverage: f64) -> Value {
    // 整篇 source 只分词一次，逐字段复用（N 个字段 = N 次对齐、1 次分词）
    let src_tokens = tokenize_tokens(source);
    let mut map = Map::new();
    collect_provenance(
        &src_tokens,
        source,
        value,
        String::new(),
        min_coverage,
        &mut map,
    );
    Value::Object(map)
}

fn collect_provenance(
    src_tokens: &[Token],
    source: &str,
    value: &Value,
    path: String,
    min_coverage: f64,
    out: &mut Map<String, Value>,
) {
    match value {
        Value::Object(m) => {
            for (k, v) in m {
                if k == "_provenance" {
                    continue;
                }
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                collect_provenance(src_tokens, source, v, child, min_coverage, out);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                collect_provenance(
                    src_tokens,
                    source,
                    v,
                    format!("{path}[{i}]"),
                    min_coverage,
                    out,
                );
            }
        }
        Value::String(s) => {
            let key = if path.is_empty() {
                "$".to_string()
            } else {
                path
            };
            let entry = match align_tokens(src_tokens, &tokenize_tokens(s), source, s, min_coverage)
            {
                Some(m) => json!({
                    "start": m.interval.start,
                    "end": m.interval.end,
                    "coverage": m.coverage,
                }),
                None => Value::Null,
            };
            out.insert(key, entry);
        }
        _ => {}
    }
}

/// 把溯源映射挂到结果 Value 的 `_provenance` 键（与业务字段并列）。
///
/// 非 JSON 对象结果（根数组/标量）无法并列挂载 → 显性 warn 并跳过；
/// 已存在 `_provenance` 键时 warn 覆盖。
pub fn attach_provenance(result: &mut Value, provenance: &Value) {
    match result {
        Value::Object(map) => {
            if map.contains_key("_provenance") {
                log::warn!("provenance: result already has _provenance key; overwriting");
            }
            map.insert("_provenance".to_string(), provenance.clone());
        }
        _ => {
            log::warn!("provenance: result is not a JSON object; skip attaching _provenance");
        }
    }
}

/// 溯源接线一步式入口（供 `extract_data_with_provenance` 复用）：
/// 关闭 → `(result, None)` 零开销短路；开启 → 计算映射并挂 `_provenance`
/// （空映射不挂载），返回 `(result, Some(map))`。
pub fn apply_provenance(
    source: &str,
    mut result: Value,
    cfg: &ProvenanceConfig,
) -> (Value, Option<Value>) {
    if !cfg.enabled {
        return (result, None);
    }
    let map = provenance_map(source, &result, cfg.min_coverage);
    let non_empty = map.as_object().is_some_and(|m| !m.is_empty());
    if non_empty {
        attach_provenance(&mut result, &map);
    }
    (result, Some(map))
}

/// 按 char 偏移切取原文字符串（[`CharInterval`] 的安全读取器）：
/// Rust 字符串切片是字节语义，直接 `source[start..end]` 在多字节字符上会 panic
pub fn slice_char_range(source: &str, interval: &CharInterval) -> String {
    source
        .chars()
        .skip(interval.start)
        .take(interval.end.saturating_sub(interval.start))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== tokenize =====

    #[test]
    fn tokenize_lowercases_and_reports_char_offsets() {
        let toks = tokenize("Hello 世界, Rust Crawler");
        assert_eq!(
            toks,
            vec![
                ("hello".to_string(), 0),
                ("世界".to_string(), 6),
                ("rust".to_string(), 10),
                ("crawler".to_string(), 15),
            ]
        );
    }

    #[test]
    fn tokenize_cjk_contiguous_run_is_single_token() {
        let toks = tokenize("今天天气很好，我们去公园散步。");
        assert_eq!(
            toks,
            vec![
                ("今天天气很好".to_string(), 0),
                ("我们去公园散步".to_string(), 7),
            ]
        );
    }

    // ===== align：精确子串（含 CJK）=====

    #[test]
    fn align_exact_ascii_substring_returns_correct_interval() {
        let source = "The quick brown fox jumps over the lazy dog.";
        let m = align(source, "quick brown fox", DEFAULT_MIN_COVERAGE)
            .expect("exact substring must align");
        assert_eq!(m.coverage, 1.0);
        assert_eq!(slice_char_range(source, &m.interval), "quick brown fox");
    }

    #[test]
    fn align_exact_cjk_substring_char_offsets_safe() {
        let source = "今天天气很好，我们去公园散步。";
        let m =
            align(source, "今天天气很好", DEFAULT_MIN_COVERAGE).expect("CJK substring must align");
        assert_eq!(m.coverage, 1.0);
        assert_eq!(m.interval.start, 0);
        assert_eq!(m.interval.end, 6);
        assert_eq!(slice_char_range(source, &m.interval), "今天天气很好");
    }

    #[test]
    fn align_cjk_substring_in_middle_char_offsets_not_bytes() {
        let source = "前言三字。今天天气很好，后文。";
        let m = align(source, "今天天气很好", DEFAULT_MIN_COVERAGE).expect("CJK middle substring");
        assert_eq!(slice_char_range(source, &m.interval), "今天天气很好");
    }

    // ===== align：词序扰动（LLM 改写虚词）=====

    #[test]
    fn align_tolerates_reworded_excerpt_when_coverage_passes_gate() {
        // 摘录改写了 "the" → "a"，其余词按序保留：4/5 = 0.8 ≥ 0.75
        let source = "The quick brown fox jumps over the lazy dog.";
        let m = align(source, "a quick brown fox jumps", DEFAULT_MIN_COVERAGE)
            .expect("majority-ordered rewrite must align");
        assert!(m.coverage >= 0.75, "coverage = {}", m.coverage);
        assert_eq!(
            slice_char_range(source, &m.interval),
            "quick brown fox jumps"
        );
    }

    #[test]
    fn align_tolerates_missing_tokens_with_dense_interval() {
        // 摘录 4 词中 3 词按序命中（"gamma" 在原文缺失）：0.75 过门；区间仍紧
        let source = "alpha beta delta epsilon";
        let m = align(source, "alpha beta gamma delta", DEFAULT_MIN_COVERAGE)
            .expect("3/4 tokens in order");
        assert!((m.coverage - 0.75).abs() < 1e-9);
        assert_eq!(slice_char_range(source, &m.interval), "alpha beta delta");
    }

    // ===== align：乱序/幻觉 =====

    #[test]
    fn align_scattered_tokens_fail_density_gate() {
        // 两个词都出现但相距过远：密度 < 1/3，不硬凑区间
        let source = "alpha a1 a2 a3 a4 a5 a6 a7 a8 beta";
        let m = align(source, "alpha beta", DEFAULT_MIN_COVERAGE);
        assert!(m.is_none(), "sparse interval must be rejected");
    }

    #[test]
    fn align_hallucinated_excerpt_returns_none() {
        let source = "The quick brown fox jumps over the lazy dog.";
        assert!(align(
            source,
            "purple elephant unicorn riding",
            DEFAULT_MIN_COVERAGE
        )
        .is_none());
        assert!(align(
            source,
            " completely fabricated words here ",
            DEFAULT_MIN_COVERAGE
        )
        .is_none());
    }

    // ===== align：覆盖率门边界 =====

    /// 100 词摘录：74 词命中 → coverage 0.74 拒绝；75 词命中 → 0.75 通过
    #[test]
    fn align_coverage_gate_boundary_rejects_0_74_accepts_0_75() {
        let source_of = |words: usize| {
            let mut source = String::new();
            for i in 0..words {
                source.push_str(&format!("w{i:03} f{i:03} "));
            }
            source
        };
        // 摘录固定 100 词：w000..w099
        let excerpt: String = (0..100)
            .map(|i| format!("w{i:03}"))
            .collect::<Vec<_>>()
            .join(" ");

        // 原文含 w000..w074（75 词命中）→ coverage 0.75 过门
        let source_75 = source_of(75);
        let hit = align(&source_75, &excerpt, DEFAULT_MIN_COVERAGE).expect("0.75 must pass");
        assert!((hit.coverage - 0.75).abs() < 1e-9);
        assert!(slice_char_range(&source_75, &hit.interval).starts_with("w000"));

        // 原文含 w000..w073（74 词命中）→ coverage 0.74 被拒
        let source_74 = source_of(74);
        let miss = align(&source_74, &excerpt, DEFAULT_MIN_COVERAGE);
        assert!(miss.is_none(), "0.74 must be rejected by coverage gate");
    }

    // ===== align：密度门边界 =====

    #[test]
    fn align_density_gate_boundary() {
        // 密度 10/34 ≈ 0.294 < 1/3 → 拒绝
        let sparse = "alpha a1 a2 a3 a4 a5 a6 a7 a8 beta";
        assert!(align(sparse, "alpha beta", DEFAULT_MIN_COVERAGE).is_none());
        // 密度 10/12 ≈ 0.83 ≥ 1/3 → 通过
        let dense = "alpha x beta";
        let m = align(dense, "alpha beta", DEFAULT_MIN_COVERAGE).expect("dense interval passes");
        assert_eq!(slice_char_range(dense, &m.interval), "alpha x beta");
    }

    // ===== align：重复词歧义 =====

    #[test]
    fn align_duplicate_phrase_takes_first_monotone_interval() {
        let source = "the quick brown fox naps. later the quick brown fox returns.";
        let m = align(source, "quick brown fox", DEFAULT_MIN_COVERAGE).expect("duplicate phrase");
        assert_eq!(slice_char_range(source, &m.interval), "quick brown fox");
        assert_eq!(m.interval.start, 4, "首个单调可行区间（确定性）");
    }

    // ===== align：空输入与超长源 =====

    #[test]
    fn align_empty_inputs_return_none_without_panic() {
        assert!(align("", "anything", DEFAULT_MIN_COVERAGE).is_none());
        assert!(align("anything", "", DEFAULT_MIN_COVERAGE).is_none());
        assert!(align("", "", DEFAULT_MIN_COVERAGE).is_none());
        assert!(align("   \n\t ", "  ", DEFAULT_MIN_COVERAGE).is_none());
        assert!(align("no word tokens ??? !!!", "___", DEFAULT_MIN_COVERAGE).is_none());
    }

    /// 性能冒烟：5 万字符原文 × 精确摘录必须在秒级内完成（贪心 O(n+m)）
    #[test]
    fn align_50k_char_source_completes_fast() {
        let unit = "alpha beta gamma delta ";
        let source = unit.repeat(2400); // ≈ 52_800 chars
        let excerpt = "gamma delta alpha beta";
        let start = std::time::Instant::now();
        let m = align(&source, excerpt, DEFAULT_MIN_COVERAGE).expect("exact phrase present");
        let elapsed = start.elapsed();
        assert_eq!(slice_char_range(&source, &m.interval), excerpt);
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "elapsed = {elapsed:?}"
        );
    }

    /// 性能冒烟：散乱摘录触发第二层时，窗口词元数超限必须放弃 DP（不失控）
    #[test]
    fn align_50k_char_source_scattered_tokens_bounded() {
        let unit = "alpha beta gamma delta ";
        let source = unit.repeat(2400);
        let excerpt = "delta zeta gamma beta"; // zeta 不存在 → 剪枝/DP 有界
        let start = std::time::Instant::now();
        let _ = align(&source, excerpt, DEFAULT_MIN_COVERAGE);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "scattered align must stay bounded, took {:?}",
            start.elapsed()
        );
    }

    // ===== provenance_map =====

    #[test]
    fn provenance_map_nested_object_and_array_paths() {
        let source = "Alice wrote Rust in Action in 2023. Chapters: intro, setup.";
        let value = json!({
            "author": "Alice",
            "book": { "title": "Rust in Action", "year": 2023 },
            "chapters": ["intro", "setup"]
        });
        let map = provenance_map(source, &value, DEFAULT_MIN_COVERAGE);
        let obj = map.as_object().expect("object map");

        let author = &obj["author"];
        assert_eq!(author["start"], 0);
        assert_eq!(author["end"], 5);
        assert_eq!(author["coverage"], 1.0);

        assert_eq!(obj["book.title"]["start"], 12);
        assert_eq!(obj["book.title"]["coverage"], 1.0);

        assert_eq!(obj["chapters[0]"]["coverage"], 1.0);
        assert_eq!(obj["chapters[1]"]["coverage"], 1.0);
        // 数字叶子不产条目
        assert!(!obj.contains_key("book.year"));
    }

    #[test]
    fn provenance_map_failed_string_fields_are_null() {
        let source = "plain source text only";
        let value = json!({ "present": "source text", "hallucinated": "no such words" });
        let map = provenance_map(source, &value, DEFAULT_MIN_COVERAGE);
        assert!(map["present"].is_object(), "命中字段产区间");
        assert!(map["hallucinated"].is_null(), "全败字段值为 null");
    }

    #[test]
    fn provenance_map_root_string_uses_dollar_path_and_skips_provenance_key() {
        let map = provenance_map("hello world", &json!("hello world"), DEFAULT_MIN_COVERAGE);
        assert_eq!(map["$"]["coverage"], 1.0);

        let value = json!({ "_provenance": { "x": 1 }, "text": "hello" });
        let map = provenance_map("hello world", &value, DEFAULT_MIN_COVERAGE);
        assert!(!map.as_object().unwrap().contains_key("_provenance"));
        assert!(map["text"].is_object());
    }

    // ===== attach / apply =====

    #[test]
    fn attach_provenance_merges_alongside_business_fields() {
        let mut result = json!({"title": "t"});
        let prov = json!({"title": {"start": 0, "end": 1, "coverage": 1.0}});
        attach_provenance(&mut result, &prov);
        assert_eq!(result["_provenance"], prov);
        assert_eq!(result["title"], "t");
    }

    #[test]
    fn attach_provenance_non_object_result_is_skipped() {
        let mut result = json!(["a"]);
        attach_provenance(&mut result, &json!({}));
        assert_eq!(result, json!(["a"]), "非对象结果不挂载");
    }

    #[test]
    fn apply_provenance_disabled_short_circuits_to_none() {
        let cfg = ProvenanceConfig::default();
        assert!(!cfg.enabled);
        let (value, prov) = apply_provenance("source", json!({"a": "source"}), &cfg);
        assert_eq!(value, json!({"a": "source"}));
        assert!(prov.is_none());
        assert!(value.get("_provenance").is_none());
    }

    #[test]
    fn apply_provenance_enabled_attaches_map() {
        let cfg = ProvenanceConfig {
            enabled: true,
            min_coverage: DEFAULT_MIN_COVERAGE,
        };
        let (value, prov) = apply_provenance("hello world", json!({"a": "world"}), &cfg);
        assert!(value["_provenance"].is_object());
        assert!(value["_provenance"]["a"].is_object());
        let map = prov.expect("enabled yields Some(map)");
        assert!(map["a"].is_object());
    }

    #[test]
    fn apply_provenance_empty_map_not_attached() {
        let cfg = ProvenanceConfig {
            enabled: true,
            min_coverage: DEFAULT_MIN_COVERAGE,
        };
        let (value, prov) = apply_provenance("source", json!({"count": 3}), &cfg);
        assert!(value.get("_provenance").is_none(), "空映射不挂载");
        let map = prov.expect("enabled yields Some(map)");
        assert_eq!(map, json!({}));
    }

    // ===== slice_char_range =====

    #[test]
    fn slice_char_range_is_cjk_safe() {
        let source = "今天天气很好，我们去公园散步。";
        let iv = CharInterval { start: 7, end: 14 };
        assert_eq!(slice_char_range(source, &iv), "我们去公园散步");
    }

    #[test]
    fn slice_char_range_out_of_bounds_clamps() {
        assert_eq!(
            slice_char_range("abc", &CharInterval { start: 1, end: 99 }),
            "bc"
        );
        assert_eq!(
            slice_char_range("abc", &CharInterval { start: 5, end: 9 }),
            ""
        );
    }

    // ===== ProvenanceConfig =====

    #[test]
    fn provenance_config_default_is_disabled_with_spec_gate() {
        let cfg = ProvenanceConfig::default();
        assert!(!cfg.enabled);
        assert!((cfg.min_coverage - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn provenance_config_from_settings_maps_both_keys() {
        let mut settings = crate::config::settings::LLMSettings::default();
        settings.provenance_enabled = true;
        settings.provenance_min_coverage = 0.6;
        let cfg = ProvenanceConfig::from_settings(&settings);
        assert!(cfg.enabled);
        assert!((cfg.min_coverage - 0.6).abs() < f64::EPSILON);
    }

    // ===== helpers =====

    /// 门限内允许不命中词元数（与 align 内部换算一致，供测试构造用例）
    #[allow(dead_code)]
    fn allowed_miss(m: usize, min_coverage: f64) -> usize {
        m - ((min_coverage * m as f64).ceil() as usize).clamp(1, m)
    }
}
