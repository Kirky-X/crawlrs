// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 转换器装配面单测（从 mod.rs 外置：仓库钩子禁止 mod.rs 内实现体）

use crate::config::settings::MarkdownSettings;
use crate::domain::services::markdown::build_converter;

#[test]
fn build_converter_default_returns_alchemize() {
    let settings = MarkdownSettings::default();
    assert_eq!(build_converter(&settings).name(), "alchemize");
}

#[test]
fn build_converter_htmd_returns_htmd_direct() {
    let settings = MarkdownSettings {
        converter: "htmd".to_string(),
    };
    assert_eq!(build_converter(&settings).name(), "htmd");
}

#[test]
fn build_converter_invalid_fail_closed_to_alchemize() {
    // 配置校验期已拒绝非法值；此分支防御编程绕过校验的调用路径
    let settings = MarkdownSettings {
        converter: "banana".to_string(),
    };
    assert_eq!(build_converter(&settings).name(), "alchemize");
}

#[test]
fn build_converter_value_is_case_insensitive() {
    let settings = MarkdownSettings {
        converter: "ALCHEMIZE".to_string(),
    };
    assert_eq!(build_converter(&settings).name(), "alchemize");
}
