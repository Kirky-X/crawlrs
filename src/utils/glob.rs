// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 统一 `*`/`?` 通配匹配（单实现，多调用点复用）
//!
//! 语义：`*` 匹配任意长度序列（含 `/`——URL 场景非路径感知）、`?` 恰好匹配
//! 单个字符、其余字符按字面量比较。
//!
//! 实现：迭代双指针 + 最近星号回溯（经典 greedy glob），最坏 O(len(pattern)
//! × len(text))，无递归回溯的指数放大。此前 application（map_use_case）与
//! engines（xhr_capture）各持一份同语义实现，收敛到本模块防漂移。

/// `*`（任意序列）/`?`（单字符）通配匹配
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pos, mut text_mark) = (usize::MAX, 0usize);
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == '?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star_pos = pi;
            text_mark = ti;
            pi += 1;
        } else if star_pos != usize::MAX {
            pi = star_pos + 1;
            text_mark += 1;
            ti = text_mark;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }
    pi == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_spans_separators_and_empty() {
        assert!(glob_match("*", "anything at all"));
        assert!(glob_match("*/api/*", "https://example.com/api/data"));
        assert!(glob_match("*/data", "https://x.com/a/b/data"));
        assert!(glob_match("https://x.com/*", "https://x.com/"));
        assert!(!glob_match("https://x.com/*", "https://x.com"));
    }

    #[test]
    fn question_mark_requires_exactly_one_char() {
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(!glob_match("a?c", "abbc"));
    }

    #[test]
    fn empty_pattern_only_matches_empty() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "x"));
    }

    #[test]
    fn multi_star_backtracks_linearly_and_correctly() {
        // 多 `*` 模式：验证回溯正确性（实现保证 O(n×m)，无指数递归）
        assert!(glob_match("*a*b*c", "xa xb xc"));
        assert!(glob_match("*a*a*a*b", "aaab"));
        assert!(!glob_match("*a*a*a*b", "aaa"));
        assert!(glob_match(
            "a*a*a*a*a*a*a*a*a*a*a",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        assert!(!glob_match(
            "a*a*a*a*a*a*a*a*a*a*a*b",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
    }

    #[test]
    fn regex_like_metachars_are_literal() {
        assert!(glob_match("*[id=1]*", "https://x.com/[id=1]"));
        assert!(!glob_match("*[id=1]*", "https://x.com/item"));
    }
}
