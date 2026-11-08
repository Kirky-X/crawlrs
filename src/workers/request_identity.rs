// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 请求身份指纹（`ScrapeOptions` 内容相关字段的确定性编码）
//!
//! 本模块是全仓唯一的 `ScrapeOptions` 身份编码实现：抓取缓存键
//! （[`crate::workers::cache_utils::generate_scrape_cache_key`]）直接复用
//! [`request_fingerprint`]，合并键（[`crate::workers::coalesce_coordinator::CoalesceKey`]）
//! 复用 [`coalesce_fingerprint`]，不得另建字段清单。`ScrapeOptions` 新增字段会
//! 自动进入缓存键与合并键，避免两条链路对"请求身份"的定义漂移（控制开关类字段
//! 按下方归一例外处理）。
//!
//! 指纹实现依赖平台/服务端构建面（`engines` + `common`），因此位于 `workers`
//! 层；`utils::coalesce` 只保留与业务无关的 single-flight 泛型原语。

use std::collections::HashMap;
use std::hash::Hasher;

use crate::common::CacheMode;
use crate::engines::types::ScrapeOptions;

/// 计算请求身份（`ScrapeOptions` 内容相关字段）的确定性指纹，供**缓存键**使用
///
/// 编码分两步：
/// 1. `serde_json::to_value` 把 `ScrapeOptions` 转成结构化 JSON 表示；headers
///    单独替换为「小写名 + 值排序的二元组数组」，消除 `HashMap` 迭代序与
///    header 名大小写差异；`cache_mode` 归一为 `null`
/// 2. 递归按键排序所有 JSON 对象后 `serde_json::to_vec` 得到确定性字节串，
///    再经 `DefaultHasher`（SipHash-1-3）压缩为 64 位
///
/// # cache_mode 不参与缓存身份
///
/// `cache_mode` 是缓存/合并的控制开关，不是内容身份：`None` 与
/// `Some(CacheMode::Enabled)` 语义等价（默认启用），`ReadOnly` / `Bypass` /
/// `Disabled` 只改变本次读写字行为，不改变抓取结果。若纳入指纹，会同时造成：
/// - `ReadOnly` 读取与写入使用不同 key → 永远 miss
/// - `Bypass` 写入的条目无人读取（读取 key 不同）
/// - `None` 与 `Some(Enabled)` 生成双 key，缓存与合并都被拆散
///
/// 因此规范化投影把该字段固定为 `null`，四种模式得到同一缓存键。
/// 合并键改用 [`coalesce_fingerprint`]，在该投影之上叠加由 `cache_mode`
/// 派生的 `cache_read_allowed` 布尔字段（见该函数文档）。
///
/// 该归一不是第二份字段清单：除 headers 的确定性替换外，其余字段仍由
/// `ScrapeOptions` 的 `Serialize` 自动纳入。
///
/// 选 `DefaultHasher` 而非自研 FNV：键只存在于进程内存，无须跨进程稳定；
/// 结构化序列化已保证编码确定，标准库零依赖即可。
#[must_use]
pub(crate) fn request_fingerprint(options: &ScrapeOptions) -> u64 {
    hash_identity(request_identity(options))
}

/// 计算**合并键**指纹：缓存身份投影 + `cache_read_allowed` 布尔投影
///
/// 等待方能否安全收养 leader 结果，取决于等待方是否允许读缓存：
/// - `Enabled` / `None` / `ReadOnly`（允许读缓存）→ `true`
/// - `Bypass` / `Disabled`（禁止读缓存，必须新鲜抓取）→ `false`
///
/// 若所有模式共用同一合并键，`Bypass`/`Disabled` 请求可能收养 leader 从缓存
/// 命中得到的结果，违反"跳过缓存、必须新鲜抓取"的语义。因此在缓存身份
/// （`cache_mode` 已归一为 `null`）之上叠加该布尔字段：两种"不可读缓存"
/// 模式与三种"可读缓存"模式分成两个合并键。
///
/// 字段仅在**禁止读缓存**时插入 `false`；可读缓存模式不插入该字段（默认
/// 语义即允许读取）。等价类与始终插入 `true` 的实现完全一致，只省掉一次
/// 默认值编码。
///
/// 布尔字段由 `cache_mode` 派生，不是新增的字段清单；缓存键仍走
/// [`request_fingerprint`]，四种模式保持同一键。
#[must_use]
pub(crate) fn coalesce_fingerprint(options: &ScrapeOptions) -> u64 {
    const CACHE_READ_ALLOWED_FIELD: &str = "cache_read_allowed";

    let mut identity = request_identity(options);
    if !cache_read_allowed(options.cache_mode) {
        if let serde_json::Value::Object(map) = &mut identity {
            map.insert(
                CACHE_READ_ALLOWED_FIELD.to_string(),
                serde_json::Value::Bool(false),
            );
        }
    }
    hash_identity(identity)
}

/// 该缓存模式是否允许读缓存（由 `CacheMode::should_read` 语义派生）
fn cache_read_allowed(mode: Option<CacheMode>) -> bool {
    !matches!(mode, Some(CacheMode::Bypass | CacheMode::Disabled))
}

/// 构造 `ScrapeOptions` 的规范化身份 JSON：headers 确定性编码 + `cache_mode` 归一为 null
fn request_identity(options: &ScrapeOptions) -> serde_json::Value {
    // 字段名必须与 `ScrapeOptions` 的字段名一致；字段改名时须同步更新此处。
    // - `headers`：按确定性结构编码，消除 HashMap 迭代序
    // - `cache_mode`：控制开关归一为 null，不参与内容身份（见 request_fingerprint）
    const HEADERS_FIELD: &str = "headers";
    const CACHE_MODE_FIELD: &str = "cache_mode";

    let mut identity =
        serde_json::to_value(options).expect("ScrapeOptions JSON serialization is infallible");
    if let serde_json::Value::Object(map) = &mut identity {
        map.insert(
            HEADERS_FIELD.to_string(),
            canonical_headers(&options.headers),
        );
        map.insert(CACHE_MODE_FIELD.to_string(), serde_json::Value::Null);
    }
    identity
}

/// 把规范化身份 JSON 压缩为 64 位确定性指纹
fn hash_identity(identity: serde_json::Value) -> u64 {
    let canonical = canonicalize_json(identity);
    let bytes =
        serde_json::to_vec(&canonical).expect("canonical JSON value serialization is infallible");

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(&bytes);
    hasher.finish()
}

/// 把 headers 编码为「小写名 + 值」排序的二元组数组
///
/// 先小写名（RFC 9110 大小写不敏感），再按（名, 值）排序：同名不同大小写的
/// header 也能得到与 `HashMap` 迭代序无关的确定编码。
fn canonical_headers(headers: &HashMap<String, String>) -> serde_json::Value {
    use serde_json::Value;

    let mut pairs: Vec<(String, &str)> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.as_str()))
        .collect();
    pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    Value::Array(
        pairs
            .into_iter()
            .map(|(name, value)| {
                Value::Array(vec![Value::String(name), Value::String(value.to_owned())])
            })
            .collect(),
    )
}

/// 递归按键排序 JSON 对象（数组保持原序）
///
/// `serde_json` 启用 `preserve_order` 时 `Map` 保留插入序；本函数显式重建
/// 有序对象，使编码不依赖字段插入顺序或 `HashMap` 迭代序。
fn canonicalize_json(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;

    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            let mut sorted = serde_json::Map::with_capacity(entries.len());
            for (key, value) in entries {
                sorted.insert(key, canonicalize_json(value));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize_json).collect()),
        scalar => scalar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认选项的指纹稳定（跨调用一致）
    #[test]
    fn request_fingerprint_is_stable() {
        let first = request_fingerprint(&ScrapeOptions::default());
        let second = request_fingerprint(&ScrapeOptions::default());
        assert_eq!(first, second);
    }

    /// 可读缓存的三种模式得到同一合并键；禁止读缓存的两模式彼此相同、与可读分离
    #[test]
    fn coalesce_fingerprint_groups_cache_modes() {
        fn fp(mode: Option<CacheMode>) -> u64 {
            coalesce_fingerprint(&ScrapeOptions {
                cache_mode: mode,
                ..Default::default()
            })
        }

        let readable = fp(None);
        assert_eq!(fp(Some(CacheMode::Enabled)), readable);
        assert_eq!(fp(Some(CacheMode::ReadOnly)), readable);
        assert_eq!(fp(Some(CacheMode::Bypass)), fp(Some(CacheMode::Disabled)));
        assert_ne!(fp(Some(CacheMode::Bypass)), readable);
    }

    /// L3 回归：可读缓存模式不插入 `cache_read_allowed` 默认字段，
    /// 但等价类与"始终插入 true"保持一致（仅禁止读缓存时插入 false）
    #[test]
    fn coalesce_fingerprint_omits_default_true_projection() {
        // 可读模式的合并身份与缓存身份逐字节一致（未插入投影字段）
        assert_eq!(
            coalesce_fingerprint(&ScrapeOptions::default()),
            request_fingerprint(&ScrapeOptions::default()),
            "readable mode must not add a default projection field"
        );
        assert_ne!(
            coalesce_fingerprint(&ScrapeOptions {
                cache_mode: Some(CacheMode::Bypass),
                ..Default::default()
            }),
            request_fingerprint(&ScrapeOptions::default()),
            "non-readable mode must add the false projection field"
        );
    }

    /// headers 名大小写归一、值大小写敏感；顺序无关
    #[test]
    fn coalesce_fingerprint_handles_headers_deterministically() {
        let mut a = ScrapeOptions::default();
        a.headers.insert("X-Token".into(), "V".into());
        a.headers.insert("accept".into(), "text/html".into());
        let mut b = ScrapeOptions::default();
        b.headers.insert("ACCEPT".into(), "text/html".into());
        b.headers.insert("x-token".into(), "V".into());
        assert_eq!(coalesce_fingerprint(&a), coalesce_fingerprint(&b));

        let mut lower_value = ScrapeOptions::default();
        lower_value.headers.insert("X-Token".into(), "v".into());
        assert_ne!(coalesce_fingerprint(&a), coalesce_fingerprint(&lower_value));
    }
}
