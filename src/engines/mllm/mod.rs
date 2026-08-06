// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! MLLM 自主导航爬取引擎
//!
//! 使用视觉大模型（MLLM）分析页面截图，自主决策导航操作（点击/滚动/输入），
//! 实现 agentic loop 式的智能爬取。由 `engine-mllm` feature 门控。

pub mod action_executor; // T051：动作执行器
pub mod config;
pub mod decision;
