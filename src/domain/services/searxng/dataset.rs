// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

// UNCONFIRMED-SCHEMA: searx.space/data/instances.json 的字段 schema 未经真实样本验证
// （本部署网络实测不可达，2026-10-17）。字段模型按 searxng/searx-space 生成脚本的
// 已知形状建模并全字段宽容（`Option` + `serde(default)`，未知字段忽略，单条目解析
// 失败降级为默认元数据而非整体崩溃）；运行时以实例 JSON API 探活为权威可用信号，
// 数据集元数据仅做准入预过滤。真实样本固化后应替换 tests/fixtures/searxng/
// instances_sample.json 并移除本申报（见 change searxng-meta-search 的 T004）。

//! searx.space 数据集解析与准入谓词
//!
//! 数据集（`https://searx.space/data/instances.json`）是公共 SearXNG 实例的登记出口：
//! 顶层 `instances` 是 `url -> entry` 的映射。本模块只做两件事：
//!
//! 1. **宽容解析**：schema 未固定（见上方 UNCONFIRMED-SCHEMA），所有字段可缺、
//!    可多，单个条目类型不符时降级为「无元数据」而不是整体失败；
//! 2. **准入预过滤**：HTTPS && 非内网 && `network_type == "normal"`（字段在时）
//!    && JSON API 可用（字段在时）&& `uptime_day >= min_uptime_day`（字段在时）。
//!
//! 预算出的 [`AdmittedInstance`] 交给仓储 `upsert_from_dataset` 幂等落库，
//! 是否真的可用最终由探活（`probe.rs`）裁决。

use std::collections::HashMap;

use log::debug;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::domain::repositories::searxng_instance_repository::{
    normalize_instance_url, AdmittedInstance, InstanceUrlError,
};

/// 数据集解析错误（仅顶层结构不合法时产生）
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DatasetParseError {
    /// 顶层不是对象，或缺少 `instances` 对象
    #[error("invalid dataset payload: {0}")]
    InvalidPayload(String),
}

/// searx.space 数据集（顶层 `instances` 映射）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dataset {
    /// `url -> entry`
    #[serde(default)]
    pub instances: HashMap<String, DatasetEntry>,
}

/// 单个实例条目（全字段宽容）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatasetEntry {
    /// 网络类型（`normal` / `tor` / `ipv6` / ...）
    #[serde(default)]
    pub network_type: Option<String>,
    /// SearXNG 版本
    #[serde(default)]
    pub version: Option<String>,
    /// JSON API 可用性：可以是 bool，也可以是带 `status_code` 的详情对象；
    /// 用 `Value` 承载以容忍 schema 漂移。
    #[serde(default)]
    pub api: Option<serde_json::Value>,
    /// HTTP 面评级
    #[serde(default)]
    pub http: Option<GradedStatus>,
    /// TLS 评级
    #[serde(default)]
    pub tls: Option<GradedStatus>,
    /// CDN 评级
    #[serde(default)]
    pub cdn: Option<GradedStatus>,
    /// 在线率
    #[serde(default)]
    pub uptime: Option<Uptime>,
}

/// 带评级的状态块（`{ "status_code": 200, "grade": "A+" }`）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GradedStatus {
    /// HTTP 状态码（字段在时）
    #[serde(default)]
    pub status_code: Option<u16>,
    /// 评级字符串
    #[serde(default)]
    pub grade: Option<String>,
}

/// 在线率块（百分比，`{ "day": 99.5, "month": 98.2 }`）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Uptime {
    /// 日在线率
    #[serde(default)]
    pub day: Option<f64>,
    /// 月在线率
    #[serde(default)]
    pub month: Option<f64>,
}

/// 准入拒绝原因（显性化：采集报告按类别汇总 errors）
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AdmissionReject {
    /// 非 HTTPS 实例
    #[error("not https: {0}")]
    NotHttps(String),
    /// 内网 / 环回 / 非法 URL
    #[error("unsafe or invalid url: {0}")]
    UnsafeUrl(String),
    /// 非 normal 网络类型（tor / ipv6 ...）
    #[error("unsupported network type: {0}")]
    NonNormalNetwork(String),
    /// 数据集标注 JSON API 不可用
    #[error("json api unavailable: {0}")]
    ApiUnavailable(String),
    /// 日在线率低于准入阈值
    #[error("low uptime: {0}")]
    LowUptime(String),
}

/// 解析数据集载荷。
///
/// 宽容策略：
/// - 顶层 `instances` 缺失 → 视为空集（返回空 `Dataset`，不视为错误）；
/// - 顶层不是对象 → `InvalidPayload`；
/// - 单个条目反序列化失败 → 降级为 `DatasetEntry::default()` 并 `debug!` 记录
///   （该实例仍可凭 URL 准入，可用性由探活决定）。
pub fn parse_dataset(payload: &serde_json::Value) -> Result<Dataset, DatasetParseError> {
    let object = payload
        .as_object()
        .ok_or_else(|| DatasetParseError::InvalidPayload("top level is not an object".into()))?;
    let Some(instances) = object.get("instances") else {
        debug!("searx.space dataset has no `instances` key; treating as empty");
        return Ok(Dataset::default());
    };
    let Some(instances) = instances.as_object() else {
        return Err(DatasetParseError::InvalidPayload(
            "`instances` is not an object".into(),
        ));
    };

    let mut dataset = Dataset::default();
    for (url, raw_entry) in instances {
        let entry = match serde_json::from_value::<DatasetEntry>(raw_entry.clone()) {
            Ok(entry) => entry,
            Err(e) => {
                debug!("searx.space entry {url} metadata unparsable, using defaults: {e}");
                DatasetEntry::default()
            }
        };
        dataset.instances.insert(url.clone(), entry);
    }
    Ok(dataset)
}

/// 准入谓词：HTTPS && 非内网 && `network_type == "normal"`（字段在时）
/// && JSON API 可用（字段在时）&& `uptime_day >= min_uptime_day`（字段在时）。
///
/// 返回规范化后的 [`AdmittedInstance`]（url 强制 https、host 小写、去尾斜杠）。
pub fn admit(
    url: &str,
    entry: &DatasetEntry,
    min_uptime_day: f64,
) -> Result<AdmittedInstance, AdmissionReject> {
    // 1. HTTPS 硬要求（数据集里存在 http 站，不能升级后照收：其 JSON API 面不成立）
    match Url::parse(url).map(|u| u.scheme().to_string()) {
        Ok(scheme) if scheme == "https" => {}
        Ok(scheme) => return Err(AdmissionReject::NotHttps(format!("{url} ({scheme})"))),
        Err(e) => return Err(AdmissionReject::UnsafeUrl(format!("{url} ({e})"))),
    }

    // 2. 规范化 + SSRF 静态校验（内网/环回拒绝）
    let normalized = normalize_instance_url(url).map_err(|e| match e {
        InstanceUrlError::Unsafe(_) | InstanceUrlError::Invalid(_) => {
            AdmissionReject::UnsafeUrl(format!("{url} ({e})"))
        }
    })?;

    // 3. 网络类型（字段在时必须为 normal）
    if let Some(network_type) = entry.network_type.as_deref() {
        if !network_type.eq_ignore_ascii_case("normal") {
            return Err(AdmissionReject::NonNormalNetwork(format!(
                "{normalized} ({network_type})"
            )));
        }
    }

    // 4. JSON API 可用性（字段在时）
    if let Some(api) = &entry.api {
        if !api_flag_available(api) {
            return Err(AdmissionReject::ApiUnavailable(normalized));
        }
    }

    // 5. 在线率阈值（字段在时；缺失视为未知，交给探活）
    if let Some(day) = entry.uptime.as_ref().and_then(|u| u.day) {
        if day < min_uptime_day {
            return Err(AdmissionReject::LowUptime(format!(
                "{normalized} (day={day} < {min_uptime_day})"
            )));
        }
    }

    Ok(AdmittedInstance {
        url: normalized,
        network_type: entry.network_type.clone(),
        version: entry.version.clone(),
        api_grade: api_grade(entry.api.as_ref()),
        http_grade: entry.http.as_ref().and_then(|g| g.grade.clone()),
        tls_grade: entry.tls.as_ref().and_then(|g| g.grade.clone()),
        cdn_grade: entry.cdn.as_ref().and_then(|g| g.grade.clone()),
        uptime_day: entry.uptime.as_ref().and_then(|u| u.day),
        uptime_month: entry.uptime.as_ref().and_then(|u| u.month),
    })
}

/// `api` 字段的可用性判读：`true` / `{ "status_code": 200 }` 视为可用；
/// `false` / 非 200 状态码视为不可用；其余形状（字符串等）视为未知→放行，
/// 由探活做权威判定。
fn api_flag_available(api: &serde_json::Value) -> bool {
    match api {
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Object(map) => match map.get("status_code").and_then(|v| v.as_u64()) {
            Some(code) => code == 200,
            None => true,
        },
        _ => true,
    }
}

/// `api` 字段的评级文本（`{ "grade": "A" }` 形态时取 grade）
fn api_grade(api: Option<&serde_json::Value>) -> Option<String> {
    api.and_then(|v| v.as_object())
        .and_then(|m| m.get("grade"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dataset() -> Dataset {
        let raw = include_str!("../../../../tests/fixtures/searxng/instances_sample.json");
        let payload: serde_json::Value = serde_json::from_str(raw).expect("fixture is valid json");
        parse_dataset(&payload).expect("fixture parses")
    }

    fn admit_url(dataset: &Dataset, url: &str) -> Result<AdmittedInstance, AdmissionReject> {
        let entry = dataset
            .instances
            .get(url)
            .unwrap_or_else(|| panic!("fixture missing entry {url}"));
        admit(url, entry, 95.0)
    }

    // ========== 解析宽容性 ==========

    #[test]
    fn test_parse_fixture_collects_all_entries() {
        let dataset = sample_dataset();
        assert_eq!(dataset.instances.len(), 10);
    }

    #[test]
    fn test_parse_unknown_extra_fields_ignored() {
        let dataset = sample_dataset();
        let entry = dataset
            .instances
            .get("https://searx.unknown-fields.example")
            .expect("entry present");
        assert_eq!(entry.network_type.as_deref(), Some("normal"));
        assert_eq!(entry.uptime.as_ref().and_then(|u| u.day), Some(99.0));
    }

    #[test]
    fn test_parse_bad_metadata_entry_falls_back_to_defaults() {
        // `http: 200` 是类型不符的元数据 → 该条目降级为默认元数据而非整体失败
        let dataset = sample_dataset();
        let entry = dataset
            .instances
            .get("https://searx.bad-metadata.example")
            .expect("entry present");
        assert!(entry.http.is_none());
        // 仍可按 URL 准入（可用性交给探活）
        assert!(admit("https://searx.bad-metadata.example", entry, 95.0).is_ok());
    }

    #[test]
    fn test_parse_missing_instances_key_yields_empty() {
        let payload = serde_json::json!({ "meta": "no instances here" });
        let dataset = parse_dataset(&payload).expect("tolerated");
        assert!(dataset.instances.is_empty());
    }

    #[test]
    fn test_parse_non_object_payload_rejected() {
        let payload = serde_json::json!([1, 2, 3]);
        assert!(matches!(
            parse_dataset(&payload),
            Err(DatasetParseError::InvalidPayload(_))
        ));
    }

    #[test]
    fn test_parse_instances_not_object_rejected() {
        let payload = serde_json::json!({ "instances": 42 });
        assert!(matches!(
            parse_dataset(&payload),
            Err(DatasetParseError::InvalidPayload(_))
        ));
    }

    // ========== 准入谓词真伪表 ==========

    #[test]
    fn test_admit_accepts_healthy_https_instance() {
        let dataset = sample_dataset();
        let admitted = admit_url(&dataset, "https://searx.working.example").expect("admitted");
        assert_eq!(admitted.url, "https://searx.working.example");
        assert_eq!(admitted.http_grade.as_deref(), Some("A+"));
        assert_eq!(admitted.tls_grade.as_deref(), Some("A+"));
        assert_eq!(admitted.cdn_grade.as_deref(), Some("A"));
        assert_eq!(admitted.uptime_day, Some(99.5));
        assert_eq!(admitted.uptime_month, Some(98.2));
        assert_eq!(admitted.version.as_deref(), Some("2024.1.1"));
    }

    #[test]
    fn test_admit_accepts_api_status_block() {
        let dataset = sample_dataset();
        let admitted = admit_url(&dataset, "https://searx.api-detail.example")
            .expect("api block 200 admitted");
        assert_eq!(admitted.api_grade.as_deref(), Some("A"));
    }

    #[test]
    fn test_admit_rejects_api_disabled() {
        let dataset = sample_dataset();
        assert!(matches!(
            admit_url(&dataset, "https://searx.api-off.example"),
            Err(AdmissionReject::ApiUnavailable(_))
        ));
    }

    #[test]
    fn test_admit_rejects_plain_http() {
        let dataset = sample_dataset();
        assert!(matches!(
            admit_url(&dataset, "http://searx.plain-http.example"),
            Err(AdmissionReject::NotHttps(_))
        ));
    }

    #[test]
    fn test_admit_rejects_low_uptime() {
        let dataset = sample_dataset();
        assert!(matches!(
            admit_url(&dataset, "https://searx.unstable.example"),
            Err(AdmissionReject::LowUptime(_))
        ));
    }

    #[test]
    fn test_admit_rejects_non_normal_network() {
        let dataset = sample_dataset();
        assert!(matches!(
            admit_url(&dataset, "https://searx.tor.example"),
            Err(AdmissionReject::NonNormalNetwork(_))
        ));
    }

    #[test]
    fn test_admit_rejects_internal_address() {
        let dataset = sample_dataset();
        assert!(matches!(
            admit_url(&dataset, "https://127.0.0.1:8888"),
            Err(AdmissionReject::UnsafeUrl(_))
        ));
    }

    #[test]
    fn test_admit_missing_optional_fields_passes() {
        let dataset = sample_dataset();
        let admitted = admit_url(&dataset, "https://searx.minimal.example").expect("admitted");
        assert!(admitted.uptime_day.is_none());
        assert!(admitted.api_grade.is_none());
    }

    #[test]
    fn test_admit_rejects_garbage_url() {
        let entry = DatasetEntry::default();
        assert!(matches!(
            admit("not-a-url", &entry, 95.0),
            Err(AdmissionReject::UnsafeUrl(_))
        ));
    }

    #[test]
    fn test_admit_normalizes_url() {
        let entry = DatasetEntry::default();
        let admitted = admit("https://EXAMPLE.com/searx/", &entry, 95.0).expect("admitted");
        assert_eq!(admitted.url, "https://example.com/searx");
    }

    #[test]
    fn test_reject_display_is_non_empty() {
        let rejects = [
            AdmissionReject::NotHttps("u".into()),
            AdmissionReject::UnsafeUrl("u".into()),
            AdmissionReject::NonNormalNetwork("u".into()),
            AdmissionReject::ApiUnavailable("u".into()),
            AdmissionReject::LowUptime("u".into()),
        ];
        for reject in rejects {
            assert!(!reject.to_string().is_empty(), "empty display: {reject:?}");
        }
    }
}
