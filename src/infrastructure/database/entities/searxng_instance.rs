// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `searxng_instances` 表实体（实例登记簿）
//!
//! 表结构由 `migrations/008_add_searxng_instances.sql` 建立；`state` / `source`
//! 以规范字符串持久化（领域枚举 `SearxngInstanceState` / `SearxngInstanceSource`
//! 负责编解码），数据集元数据字段全部可空以容忍 schema 漂移。

use dbnexus::sea_orm;
use dbnexus::sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "searxng_instances")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// 规范化 base URL（https、去尾斜杠、host 小写）
    #[sea_orm(unique)]
    pub url: String,
    /// 来源：`searx_space` / `seed`
    pub source: String,
    /// 生命周期状态：`candidate` / `active` / `cooldown` / `retired`
    pub state: String,
    /// 数据集元数据：网络类型
    pub network_type: Option<String>,
    /// 数据集元数据：SearXNG 版本
    pub version: Option<String>,
    /// 数据集元数据：JSON API 评级
    pub api_grade: Option<String>,
    /// 数据集元数据：HTTP 评级
    pub http_grade: Option<String>,
    /// 数据集元数据：TLS 评级
    pub tls_grade: Option<String>,
    /// 数据集元数据：CDN 评级
    pub cdn_grade: Option<String>,
    /// 日在线率（百分比）
    pub uptime_day: Option<f64>,
    /// 月在线率（百分比）
    pub uptime_month: Option<f64>,
    /// 最近一次成功探活时延（毫秒）
    pub latency_ms: Option<i64>,
    /// 连续失败计数
    pub consecutive_failures: i32,
    /// 最近成功时间
    pub last_success_at: Option<DateTimeWithTimeZone>,
    /// 最近失败时间
    pub last_failure_at: Option<DateTimeWithTimeZone>,
    /// 最近失败原因（截断存储）
    pub last_error: Option<String>,
    /// 连续缺席数据集的采集轮数
    pub dataset_miss_count: i32,
    /// 入库时间
    pub created_at: DateTimeWithTimeZone,
    /// 最近更新时间
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// 实体 → 领域视图
impl From<Model> for crate::domain::repositories::searxng_instance_repository::SearxngInstance {
    fn from(model: Model) -> Self {
        use crate::domain::repositories::searxng_instance_repository::{
            SearxngInstance, SearxngInstanceSource, SearxngInstanceState,
        };
        use std::str::FromStr;

        // 未知枚举值不应导致崩溃：落回最保守语义（candidate / searx_space），
        // 由后续探活重新决定状态。
        let state =
            SearxngInstanceState::from_str(&model.state).unwrap_or(SearxngInstanceState::Candidate);
        let source = SearxngInstanceSource::from_str(&model.source)
            .unwrap_or(SearxngInstanceSource::SearxSpace);

        SearxngInstance {
            url: model.url,
            source,
            state,
            network_type: model.network_type,
            version: model.version,
            api_grade: model.api_grade,
            http_grade: model.http_grade,
            tls_grade: model.tls_grade,
            cdn_grade: model.cdn_grade,
            uptime_day: model.uptime_day,
            uptime_month: model.uptime_month,
            latency_ms: model.latency_ms,
            consecutive_failures: model.consecutive_failures,
            last_success_at: model.last_success_at.map(|t| t.with_timezone(&chrono::Utc)),
            last_failure_at: model.last_failure_at.map(|t| t.with_timezone(&chrono::Utc)),
            last_error: model.last_error,
            dataset_miss_count: model.dataset_miss_count,
            created_at: model.created_at.with_timezone(&chrono::Utc),
            updated_at: model.updated_at.with_timezone(&chrono::Utc),
        }
    }
}
