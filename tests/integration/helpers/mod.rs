// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

// 只导出测试需要的函数
pub mod test_app;

pub use test_app::create_test_app;
pub use test_app::create_test_app_no_worker;
