// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Retry-After 头解析（单实现，antibot 分类器与顺序路由共用）
//!
//! 支持 RFC 7231 规定的两种形式：纯数字秒（`30`）与 HTTP 日期
//! （`Wed, 21 Oct 2015 07:28:00 GMT`）。日期形式的等待量按当前时刻折算。
//! 此前 engines/antibot（`content` 门控）与 engines/router 各持一份逐行
//! 同构实现，收敛到本模块防漂移（两者仅头部容器不同：`HeaderMap` vs
//! `HashMap`，由调用方各自提取头值）。

/// Retry-After 解析结果的钳制上限（秒）：
/// 冷却是礼貌信号而非硬门禁，超长等待由调用方的有界 gate 钳制
pub const RETRY_AFTER_MAX_SECS: u64 = 600;

/// 解析 Retry-After 头值（delta-seconds 或 HTTP 日期），返回钳制后的等待秒数
///
/// 过去的日期视为 0；数字溢出按"等待极长"处理钳到上限；空串或两种形式
/// 均不可解析时返回 `None`。
pub fn parse_retry_after_value(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let secs = if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
        raw.parse::<u64>().unwrap_or(u64::MAX)
    } else {
        let deadline = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
        let delta = deadline.with_timezone(&chrono::Utc) - chrono::Utc::now();
        u64::try_from(delta.num_seconds()).unwrap_or(0)
    };
    Some(secs.min(RETRY_AFTER_MAX_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_seconds_parsed_and_clamped() {
        assert_eq!(parse_retry_after_value("30"), Some(30));
        assert_eq!(
            parse_retry_after_value("999999"),
            Some(RETRY_AFTER_MAX_SECS),
            "超出上限钳制到 600s"
        );
    }

    #[test]
    fn http_date_converted_to_seconds() {
        // 未来 5 分钟 → 介于 299/300 之间（时钟推进容差）；
        // 日期形式的等待量同样受 600s 钳制（1 小时 futurity → 600）
        let future = chrono::Utc::now() + chrono::Duration::minutes(5);
        let raw = future.to_rfc2822();
        let secs = parse_retry_after_value(&raw).expect("future date parses");
        assert!((298..=300).contains(&secs), "got {secs}");
        let hour = chrono::Utc::now() + chrono::Duration::hours(1);
        assert_eq!(
            parse_retry_after_value(&hour.to_rfc2822()),
            Some(RETRY_AFTER_MAX_SECS),
            "date form clamped to 600s"
        );
    }

    #[test]
    fn past_date_yields_zero() {
        let past = chrono::Utc::now() - chrono::Duration::hours(1);
        assert_eq!(parse_retry_after_value(&past.to_rfc2822()), Some(0));
    }

    #[test]
    fn invalid_values_return_none() {
        assert_eq!(parse_retry_after_value(""), None);
        assert_eq!(parse_retry_after_value("  "), None);
        assert_eq!(parse_retry_after_value("soon"), None);
        assert_eq!(parse_retry_after_value("not a date"), None);
    }
}
