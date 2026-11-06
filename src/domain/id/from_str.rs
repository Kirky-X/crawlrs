// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `Id` 的字符串解析实现。
//!
//! 从 `mod.rs` 拆出：模块文件只保留类型定义与 re-export，具体实现放在独立文件。

use std::str::FromStr;

use uuid::Uuid;

use super::{Id, ParseIdError};

impl FromStr for Id {
    type Err = ParseIdError;

    /// 解析：36 位带连字符 UUID 或 32 位紧凑 UUID → u128；其余按十进制 u128 解析。
    ///
    /// **本实现是 nebulaid `from_string` 的超集**：nebulaid 只接受 36 位带连字符
    /// UUID 与十进制，不接受 32 位紧凑形态。32 位纯数字串在本实现按紧凑 UUID
    /// （hex）解释，nebulaid 按十进制解释——跨组件传递 32 位纯数字串会得到两个值，
    /// 属已知口径分歧；十进制 ID 请使用非 32 位形态。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cleaned = s.trim();
        let uuid_shaped = (cleaned.len() == 36 && cleaned.contains('-')) || cleaned.len() == 32;
        if uuid_shaped {
            if let Ok(uuid) = Uuid::parse_str(cleaned) {
                return Ok(Id(uuid.as_u128()));
            }
        }
        cleaned
            .parse::<u128>()
            .map(Id)
            .map_err(|_| ParseIdError(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_hyphenated_compact_and_decimal() {
        let uuid = Uuid::new_v4();
        let hyphenated: Id = uuid.to_string().parse().unwrap();
        assert_eq!(hyphenated.to_uuid_v8(), uuid);

        // 32 位紧凑形态与带连字符形态解析为同一值
        let compact: Id = uuid.simple().to_string().parse().unwrap();
        assert_eq!(compact, hyphenated);
        assert_eq!(compact.to_uuid_v8(), uuid);

        // 非 32 位十进制仍走数值路径
        let num: Id = "9876543210".parse().unwrap();
        assert_eq!(num.as_u128(), 9876543210u128);
    }

    #[test]
    fn test_from_str_rejects_garbage() {
        assert!("not-an-id".parse::<Id>().is_err());
        assert!("".parse::<Id>().is_err());
        assert!("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz".parse::<Id>().is_err());
    }
}
