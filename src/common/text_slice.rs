// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! UTF-8 安全截断工具。
//!
//! 历史上多处代码使用 `&s[..n]` 按字节切片，切在多字节字符（中文/emoji）
//! 中间会 panic（CWE-134 边界失效）。统一使用本模块的 char 边界安全实现。

/// 按**字符数**截断字符串：最多保留 `max_chars` 个字符。
///
/// 与 `str::chars().take(n).collect()` 等价但避免额外分配语义上的歧义；
/// `max_chars` 为 0 返回空串。
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// 返回不超过 `max_bytes` 字节且不破坏 UTF-8 边界的最长前缀（`&str`）。
///
/// 若 `max_bytes` 落在字符边界上则精确截断；否则回退到前一个边界。
pub fn safe_prefix(s: &str, max_bytes: usize) -> &str {
    if max_bytes >= s.len() {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::{safe_prefix, truncate_chars};

    #[test]
    fn truncate_chars_counts_characters_not_bytes() {
        assert_eq!(truncate_chars("hello", 3), "hel");
        // 中文每字符 3 字节：截 3 字符 = 9 字节，不会 panic
        assert_eq!(truncate_chars("你好世界", 3), "你好世");
        assert_eq!(truncate_chars(" 👍x", 2), " 👍");
        assert_eq!(truncate_chars("abc", 0), "");
        assert_eq!(truncate_chars("abc", 10), "abc");
    }

    #[test]
    fn safe_prefix_respects_char_boundary() {
        assert_eq!(safe_prefix("hello", 3), "hel");
        // max_bytes=4 落在"好"中间 → 回退到边界 3
        assert_eq!(safe_prefix("你好", 4), "你");
        assert_eq!(safe_prefix("你好", 3), "你");
        assert_eq!(safe_prefix("你好", 100), "你好");
        assert_eq!(safe_prefix("", 5), "");
    }
}
