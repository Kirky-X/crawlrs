// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 领域用例模块
///
/// 该模块包含具体的业务用例实现，每个用例代表一个完整的业务流程。
/// 领域用例协调领域对象和服务来完成特定的业务目标。
///
/// 当前的用例：
/// - 创建Webhook（create_webhook）：处理Webhook配置的创建流程
///
/// 领域用例与应用程序用例的区别在于：领域用例包含纯粹的业务逻辑，
/// 关注业务规则的实现，而应用程序用例可能包含更多的技术细节和协调逻辑。
pub mod create_webhook;
