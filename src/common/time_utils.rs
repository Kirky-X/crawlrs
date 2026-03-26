// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 时间工具模块
//!
//! 提供统一的时间戳转换功能，消除代码重复

use chrono::{DateTime, FixedOffset, Utc};

/// UTC 零时区偏移量
///
/// 使用 const fn 在编译时计算，避免运行时 unwrap
pub const UTC_OFFSET: FixedOffset = {
    // 编译时计算，避免运行时 unwrap
    match FixedOffset::east_opt(0) {
        Some(offset) => offset,
        None => panic!("UTC offset is always valid"),
    }
};

/// 将 DateTime<Utc> 转换为数据库存储格式
///
/// # Arguments
/// * `dt` - UTC 时间
///
/// # Returns
/// * FixedOffset 时间格式，用于数据库存储
#[inline]
pub fn to_db_datetime(dt: DateTime<Utc>) -> DateTime<FixedOffset> {
    dt.with_timezone(&UTC_OFFSET)
}

/// 可选时间的转换辅助函数
///
/// # Arguments
/// * `dt` - 可选的 UTC 时间
///
/// # Returns
/// * 可选的 FixedOffset 时间格式，用于数据库存储
#[inline]
pub fn to_db_datetime_opt(dt: Option<DateTime<Utc>>) -> Option<DateTime<FixedOffset>> {
    dt.map(|d| d.with_timezone(&UTC_OFFSET))
}

/// 从数据库格式转换为 DateTime<Utc>
///
/// # Arguments
/// * `dt` - FixedOffset 时间格式
///
/// # Returns
/// * UTC 时间
#[inline]
pub fn from_db_datetime(dt: DateTime<FixedOffset>) -> DateTime<Utc> {
    dt.with_timezone(&Utc)
}

/// 可选时间的反向转换辅助函数
///
/// # Arguments
/// * `dt` - 可选的 FixedOffset 时间格式
///
/// # Returns
/// * 可选的 UTC 时间
#[inline]
pub fn from_db_datetime_opt(dt: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    dt.map(|d| d.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_utc_offset_is_valid() {
        // 验证 UTC_OFFSET 是有效的零时区偏移
        assert_eq!(UTC_OFFSET, FixedOffset::east_opt(0).unwrap());
    }

    #[test]
    fn test_to_db_datetime() {
        let now = Utc::now();
        let converted = to_db_datetime(now);

        // 验证时间戳相同
        assert_eq!(now.timestamp(), converted.timestamp());
        // 验证时区是 UTC+0
        assert_eq!(converted.timezone().local_minus_utc(), 0);
    }

    #[test]
    fn test_to_db_datetime_opt_some() {
        let now = Utc::now();
        let converted = to_db_datetime_opt(Some(now));

        assert!(converted.is_some());
        let converted = converted.unwrap();
        assert_eq!(now.timestamp(), converted.timestamp());
    }

    #[test]
    fn test_to_db_datetime_opt_none() {
        let converted = to_db_datetime_opt(None);
        assert!(converted.is_none());
    }

    #[test]
    fn test_from_db_datetime() {
        let fixed_dt = FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();
        let converted = from_db_datetime(fixed_dt);

        assert_eq!(fixed_dt.timestamp(), converted.timestamp());
    }

    #[test]
    fn test_from_db_datetime_opt_some() {
        let fixed_dt = FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();
        let converted = from_db_datetime_opt(Some(fixed_dt));

        assert!(converted.is_some());
        let converted = converted.unwrap();
        assert_eq!(fixed_dt.timestamp(), converted.timestamp());
    }

    #[test]
    fn test_from_db_datetime_opt_none() {
        let converted = from_db_datetime_opt(None);
        assert!(converted.is_none());
    }

    #[test]
    fn test_roundtrip_conversion() {
        let now = Utc::now();

        // Utc -> FixedOffset -> Utc
        let to_db = to_db_datetime(now);
        let back_to_utc = from_db_datetime(to_db);

        assert_eq!(now.timestamp(), back_to_utc.timestamp());
        assert_eq!(now.timestamp_subsec_nanos(), back_to_utc.timestamp_subsec_nanos());
    }
}
