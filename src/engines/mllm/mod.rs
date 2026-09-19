// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! MLLM 自主导航爬取引擎
//!
//! 使用视觉大模型（MLLM）分析页面截图，自主决策导航操作（点击/滚动/输入），
//! 实现 agentic loop 式的智能爬取。由 `engine-mllm` feature 门控。

pub mod action_executor; // 动作执行器
pub mod config;
pub mod decision;
