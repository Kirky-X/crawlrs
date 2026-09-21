// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 反爬虫检测模块（移植 crawl4ai `antibot_detector.py` 三层检测）
//!
//! 由 `antibot` feature 门控。仅 `pub use` 公共类型——具体实现见
//! `patterns` 与 `classifier` 子模块（mod.rs 仅 re-export）。

mod classifier;
mod patterns;

pub use classifier::{classify, Detection};
pub use patterns::AntiBotTech;
