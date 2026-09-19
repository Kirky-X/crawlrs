// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 凭证掩码工具。
//!
//! 引导路径与日志不得输出完整明文凭证；需要人工辨认凭证时
//! 只展示首尾少量字符（[`mask_secret`]）。

/// 掩码一个秘密值：长度 >8 保留前 4 后 4 字符，中间以 `***` 填充；
/// 长度 ≤8（含空串）整体替换为 `***`。
///
/// 按字符而非字节处理，非 ASCII 输入不会 panic。
pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "***".to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{}***{}", head, tail)
}

#[cfg(test)]
mod tests {
    use super::mask_secret;

    #[test]
    fn masks_long_secret_keeping_head_and_tail() {
        assert_eq!(mask_secret("abcdefghijklmnop"), "abcd***mnop");
    }

    #[test]
    fn masks_short_secret_entirely() {
        assert_eq!(mask_secret("short"), "***");
        assert_eq!(mask_secret("12345678"), "***");
    }

    #[test]
    fn masks_empty_secret() {
        assert_eq!(mask_secret(""), "***");
    }

    #[test]
    fn masks_multibyte_secret_without_panic() {
        // 9 个多字节字符：按字符计数，不按字节切片
        let secret = "密密密密密密密密密";
        assert_eq!(mask_secret(secret), "密密密密***密密密密");
    }
}
