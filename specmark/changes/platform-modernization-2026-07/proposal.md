# platform-modernization-2026-07

## Motivation
crawlrs 当前依赖 4 个本地路径 crate（confers/dbnexus/limiteron/oxcache），无法独立构建，且与云端最新版脱节。同时日志层（tracing）需替换为企业级 inklog，接口层需引入 sdforge 将 Rust 接口通过 feature 封装为 HTTP 接口。gitnexus 分析显示存在幽灵函数和命名遗留问题。代码覆盖率未达 90% 目标。文档与代码存在 3 处不一致（Sea-ORM 版本、默认特性名、governor→limiteron 替换未记录）。

## Scope
1. **依赖迁移**：6 个 crate 从本地路径切换到 crates.io 最新版（oxcache 0.3.3 / dbnexus 0.2.0 / inklog 0.1.2 / sdforge 0.3.1 / confers 0.2.2 / limiteron 0.2.1），全部使用显式 features 而非 default
2. **inklog 替换 tracing**：完全移除 tracing/tracing-subscriber/tracing-appender，所有 `tracing::*!` 宏替换为 `log::*!`，日志初始化改用 inklog LoggerManager
3. **sdforge 集成**：将 Rust 接口（domain services / use cases）通过 sdforge 宏+feature 封装为 HTTP 接口，gate 在 feature flag 后
4. **幽灵函数移除**：gitnexus 深度分析，过滤 trait 实现/getter/构造器等误报，移除验证后的死代码
5. **命名修复**：gitnexus 分析错误/老旧命名调用并修复
6. **环境配置**：pangu 检查 CI/CD、Docker、env、pre-commit 完整性
7. **特性门禁**：所有功能通过 feature flag 开启，二进制不膨胀
8. **代码覆盖率**：提升到 90%+
9. **安全审计**：diting + tiangang 审计生成的代码
10. **Bug 分析**：kueiku 分析硬性 bug
11. **文档对齐**：cangjie 修复文档与代码不一致

## Non-Goals
- 不重构现有 DDD 分层架构（presentation/application/domain/infrastructure）
- 不替换 Shaku DI 框架（sdforge 用于接口封装，不替代 DI）
- 不更换 Axum Web 框架
- 不修改 examples/ 独立 workspace
- 不处理 stashes 中的历史代码（本次只处理分支清理已完成后的 main 代码）

## Clarifications
- **[Functional Scope]** Q: limiteron 是否也切换到 crates.io？
  A: 是，也切换到 crates.io v0.2.1
- **[Functional Scope]** Q: sdforge "接口封装" 具体指什么？
  A: 是将 Rust 接口通过 feature 封装成 HTTP 接口，而不是 HTTP 接口封装成 SDK
- **[Integration]** Q: inklog 如何集成？
  A: 完全替换 tracing，移除所有 tracing 宏和依赖

## NEEDS CLARIFICATION
无。所有需求已转为具体任务。
