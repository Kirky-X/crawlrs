// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 反爬虫检测模块（移植 crawl4ai `antibot_detector.py` 三层检测）
//!
//! 由 `antibot` feature 门控。仅 `pub use` 公共类型——具体实现见
//! [`patterns`] 与 [`classifier`] 子模块（规则 10：mod.rs 仅 re-export）。

mod classifier;
mod patterns;

pub use classifier::{classify, Detection};
pub use patterns::AntiBotTech;
