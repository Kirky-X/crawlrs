// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 保留期清理的分批参数（retention-worker-hardening R-retention-001/D1）。
//!
//! 纯数据结构：repo 层不读 `Settings`，`policy` 由 worker 从配置构造后传入，
//! 保持 repository 无状态（db-retention-governance Constraints 惯例）。

/// 有界删除参数：单批行数、单类单周期行数上限、单批语句超时。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionBatchPolicy {
    /// 单批最多删除行数
    pub batch_size: u64,
    /// 单类单周期删除行数上限
    pub max_rows_per_cycle: u64,
    /// 单批事务 `statement_timeout`（毫秒）
    pub statement_timeout_ms: u64,
}

impl Default for RetentionBatchPolicy {
    fn default() -> Self {
        Self {
            batch_size: 5000,
            max_rows_per_cycle: 100_000,
            statement_timeout_ms: 60_000,
        }
    }
}

impl RetentionBatchPolicy {
    /// 从 `[retention]` 配置构造（仅取删除执行相关字段；逐类超时由 worker 自持）。
    pub fn from_settings(settings: &crate::config::settings::RetentionSettings) -> Self {
        Self {
            batch_size: settings.batch_size,
            max_rows_per_cycle: settings.max_rows_per_cycle,
            statement_timeout_ms: settings.statement_timeout_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::RetentionSettings;

    /// R-retention-001：`from_settings` 逐字段映射配置值。
    #[test]
    fn from_settings_maps_each_field() {
        let settings = RetentionSettings {
            batch_size: 1234,
            max_rows_per_cycle: 56_789,
            statement_timeout_ms: 45_000,
            ..RetentionSettings::default()
        };
        let policy = RetentionBatchPolicy::from_settings(&settings);
        assert_eq!(policy.batch_size, 1234);
        assert_eq!(policy.max_rows_per_cycle, 56_789);
        assert_eq!(policy.statement_timeout_ms, 45_000);
    }

    /// R-retention-001：`Default` 与配置默认值一致（5000 / 100000 / 60000）。
    #[test]
    fn default_matches_settings_defaults() {
        let policy = RetentionBatchPolicy::default();
        assert_eq!(policy.batch_size, 5000);
        assert_eq!(policy.max_rows_per_cycle, 100_000);
        assert_eq!(policy.statement_timeout_ms, 60_000);

        let from_settings = RetentionBatchPolicy::from_settings(&RetentionSettings::default());
        assert_eq!(policy.batch_size, from_settings.batch_size);
        assert_eq!(policy.max_rows_per_cycle, from_settings.max_rows_per_cycle);
        assert_eq!(
            policy.statement_timeout_ms,
            from_settings.statement_timeout_ms
        );
    }
}
