// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

// 导出测试辅助模块
pub mod test_app;

// 导出简化版本的函数用于集成测试
#[allow(unused_imports)]
pub use test_app::create_test_app;
#[allow(unused_imports)]
pub use test_app::create_test_app_no_worker;
