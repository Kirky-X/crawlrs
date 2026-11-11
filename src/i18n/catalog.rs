// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! FTL 目录（编译期内嵌 → sdforge 注册表）。
//!
//! `locales/{en-US,zh-CN}/*.ftl` 为消息真源，经 `include_str!` 编译期内嵌，
//! 启动期行解析 `key = value` 后注册进 `sdforge::i18n` 宿主注册表
//! （`register_translation`）。内嵌保证部署产物无需携带磁盘 `locales/`
//! 目录；消息查询统一走 `sdforge::i18n::translate_for`。
//!
//! 提取口径（经全目录核实）：纯单行 `key = value` 形态，无 Fluent 选择器、
//! term、属性与多行 continuation，占位符一律 `{ $name }` 标准形——与
//! sdforge 轻量 `{ $name }` 替换语义一致（见 sdforge i18n 模块
//! `format_template`）。键位守卫（en/zh 键集合与占位符对齐、磁盘目录与
//! 内嵌清单一致）见 `tests/unit/i18n/mod.rs`。

use std::collections::BTreeSet;
use std::sync::OnceLock;

/// FTL 文件清单（不含扩展名，与 `locales/<locale>/` 下一一对应）。
/// 新增 `.ftl` 文件必须登记进此清单与 `*_SOURCES`，否则守卫测试
/// （内嵌清单 vs 磁盘目录双向一致）失败。
///
/// 运行时路径不读取本清单（`sources_for` 按 locale 维度组织），
/// 仅供守卫测试消费，故对非测试构建放行 dead_code。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const FILES: &[&str] = &[
    "api",
    "errors",
    "fluxweir",
    "logs",
    "rag",
    "validation",
    "workers",
];

// en-US（7 个，与 locales/en-US/*.ftl 一一对应）
const EN_API: &str = include_str!("../../locales/en-US/api.ftl");
const EN_ERRORS: &str = include_str!("../../locales/en-US/errors.ftl");
const EN_FLUXWEIR: &str = include_str!("../../locales/en-US/fluxweir.ftl");
const EN_LOGS: &str = include_str!("../../locales/en-US/logs.ftl");
const EN_RAG: &str = include_str!("../../locales/en-US/rag.ftl");
const EN_VALIDATION: &str = include_str!("../../locales/en-US/validation.ftl");
const EN_WORKERS: &str = include_str!("../../locales/en-US/workers.ftl");

// zh-CN（7 个，与 locales/zh-CN/*.ftl 一一对应）
const ZH_API: &str = include_str!("../../locales/zh-CN/api.ftl");
const ZH_ERRORS: &str = include_str!("../../locales/zh-CN/errors.ftl");
const ZH_FLUXWEIR: &str = include_str!("../../locales/zh-CN/fluxweir.ftl");
const ZH_LOGS: &str = include_str!("../../locales/zh-CN/logs.ftl");
const ZH_RAG: &str = include_str!("../../locales/zh-CN/rag.ftl");
const ZH_VALIDATION: &str = include_str!("../../locales/zh-CN/validation.ftl");
const ZH_WORKERS: &str = include_str!("../../locales/zh-CN/workers.ftl");

const EN_SOURCES: &[&str] = &[
    EN_API,
    EN_ERRORS,
    EN_FLUXWEIR,
    EN_LOGS,
    EN_RAG,
    EN_VALIDATION,
    EN_WORKERS,
];

const ZH_SOURCES: &[&str] = &[
    ZH_API,
    ZH_ERRORS,
    ZH_FLUXWEIR,
    ZH_LOGS,
    ZH_RAG,
    ZH_VALIDATION,
    ZH_WORKERS,
];

/// 内嵌 locale → 源清单。未登记的 locale 返回 `None`（`I18nBundle::load`
/// 据此拒绝不受支持的 locale）。
pub(crate) fn sources_for(locale: &str) -> Option<&'static [&'static str]> {
    match locale {
        "en-US" => Some(EN_SOURCES),
        "zh-CN" => Some(ZH_SOURCES),
        _ => None,
    }
}

/// 解析单源的 `key = value` 行 → `(key, value)`（值两端 trim）。
///
/// 仅取顶格 `name = …` 行：`#` 注释行与缩进 continuation 自然排除
/// （目录经核实无多行形态，行解析与完整 Fluent 解析行为一致）。
pub(crate) fn parse_entries(source: &'static str) -> Vec<(&'static str, &'static str)> {
    source
        .lines()
        .filter_map(|line| {
            if line.len() != line.trim_start().len() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let key = key.trim_end();
            if key.is_empty() || !is_message_name(key) {
                return None;
            }
            Some((key, value.trim()))
        })
        .collect()
}

/// 消息名合法性：小写字母/数字/连字符（与目录现状一致）。
fn is_message_name(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 提取值内全部 `{ $name }` 占位符名（升序去重）。
///
/// 目录占位符一律标准形（`{ $name }`，两侧空格可有可无）；
/// 非 `$` 开头的花括号段（字面量）不提取。
pub(crate) fn placeholders(value: &'static str) -> BTreeSet<&'static str> {
    let mut names = BTreeSet::new();
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        let Some(close_rel) = rest[open..].find('}') else {
            break;
        };
        let inner = rest[open + 1..open + close_rel].trim();
        if let Some(name) = inner.strip_prefix('$') {
            let name = name.trim();
            if !name.is_empty() && is_message_name(name) {
                names.insert(name);
            }
        }
        rest = &rest[open + close_rel + 1..];
    }
    names
}

/// 注册全部内嵌条目到 `sdforge::i18n` 宿主注册表（幂等，进程内一次）。
///
/// 注册表为追加覆盖语义（`register_translation` 同键后写覆盖先写），
/// 重复调用无危害；`OnceLock` 仍保证常态只注册一次。
pub fn register_all() {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        for (locale, sources) in [("en-US", EN_SOURCES), ("zh-CN", ZH_SOURCES)] {
            for source in sources {
                for (key, value) in parse_entries(source) {
                    sdforge::i18n::register_translation(locale, key, value);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_entries_extracts_key_value_pairs() {
        let entries = parse_entries(
            "# comment\nkey-a = value a\n  indented = skipped\nkey-b= value b\ninvalid key = x\n\nbare line\n",
        );
        let keys: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["key-a", "key-b"]);
        assert_eq!(entries[0].1, "value a");
        assert_eq!(entries[1].1, "value b");
    }

    #[test]
    fn placeholders_extracts_dollar_names() {
        let names = placeholders("Error: { $message } at { $url } ({$status}) no {literal}");
        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            vec!["message", "status", "url"]
        );
    }

    #[test]
    fn embedded_sources_are_non_empty_and_parseable() {
        for locale in ["en-US", "zh-CN"] {
            let sources = sources_for(locale).expect("embedded locale source");
            assert!(!sources.is_empty());
            for source in sources {
                assert!(
                    !parse_entries(source).is_empty(),
                    "embedded source for {locale} parsed to zero entries"
                );
            }
        }
    }

    #[test]
    fn unsupported_locale_has_no_sources() {
        assert!(sources_for("fr-FR").is_none());
    }

    /// 守卫：内嵌文件清单与磁盘 `locales/<locale>/` 目录双向一致
    /// （新增 .ftl 忘登记 FILES/*_SOURCES 时失败，见模块文档）。
    #[test]
    fn files_manifest_matches_disk_directory() {
        for locale in ["en-US", "zh-CN"] {
            let disk_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("locales")
                .join(locale);
            let mut disk_files: Vec<String> = std::fs::read_dir(&disk_dir)
                .unwrap_or_else(|e| panic!("read {}: {e}", disk_dir.display()))
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == "ftl"))
                .filter_map(|e| {
                    e.path()
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                })
                .collect();
            disk_files.sort();

            // FILES 与磁盘目录一一对应（清单 ↔ 目录双向）
            assert!(
                !disk_files.is_empty(),
                "disk directory {} is empty",
                disk_dir.display()
            );
            assert_eq!(
                FILES, disk_files.as_slice(),
                "locales/{locale}/*.ftl 与 catalog::FILES 不一致（新文件须同时登记 FILES 与 *_SOURCES）"
            );

            // 每个 FILES 条目都有对应内嵌源参与注册
            let sources = sources_for(locale).expect("embedded locale source");
            assert_eq!(sources.len(), FILES.len());
        }
    }
}
