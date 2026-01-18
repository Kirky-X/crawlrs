# 技术债务清理计划

本文档记录crawlrs项目的技术债务清理进度和计划。

最后更新：2025-01-18

## 清理状态概览

### ✅ 已完成

#### 1. 配置外部化 (80%完成)

**Worker数量配置化** ✅
- ✅ 添加 `WorkerSettings` 结构到 `src/config/app.rs`
- ✅ 实现自动检测CPU核心数功能
- ✅ 更新 `config/default.toml` 添加配置
- ✅ 修改 `main.rs` 从配置读取
- ✅ 添加完整测试

**Duration配置化** ✅ (基础设施完成)
- ✅ 添加 `TimeoutSettings` 及所有子结构
- ✅ 定义9个超时配置
- ✅ 更新配置文件

#### 2. 错误处理改进 (50%完成)

**自定义错误类型** ✅
- ✅ 创建 `src/domain/errors.rs` (13个错误变体)
- ✅ 创建 `src/infrastructure/errors.rs` (12个错误变体)
- ✅ 实现层间错误转换
- ✅ 添加完整测试（18个测试用例）
- ✅ 编译验证通过

### 🔄 进行中

**Domain层服务迁移**
- 识别4个需要迁移的文件：
  - `src/domain/services/crawl_service.rs`
  - `src/domain/services/team_service.rs`
  - `src/domain/services/webhook_service.rs`
  - `src/domain/services/extraction_service.rs`

**Infrastructure层服务迁移**
- 识别5个需要迁移的文件：
  - `src/infrastructure/cache/redis_client.rs`
  - `src/infrastructure/cache/cache_strategy.rs`
  - `src/infrastructure/cache/cache_manager.rs`
  - `src/infrastructure/database/repositories/scrape_result_repo_impl.rs`
  - `src/infrastructure/geolocation.rs`

### 📋 待完成

**Duration配置迁移**
- 迁移5个关键文件使用配置的超时值
- 更新相关测试

**文档更新**
- 更新 `docs/CONFIGURATION.md`
- 创建迁移指南

**CI检查**
- 添加 `anyhow` 使用检查
- 添加硬编码值检查

## 当前统计

### anyhow::Result 使用情况

**总计**: 24个文件使用 `anyhow::Result`

**Domain层** (4个文件):
- `src/domain/services/crawl_service.rs`
- `src/domain/services/team_service.rs`
- `src/domain/services/webhook_service.rs`
- `src/domain/services/extraction_service.rs`

**Infrastructure层** (5个文件):
- `src/infrastructure/cache/redis_client.rs`
- `src/infrastructure/cache/cache_strategy.rs`
- `src/infrastructure/cache/cache_manager.rs`
- `src/infrastructure/database/repositories/scrape_result_repo_impl.rs`
- `src/infrastructure/geolocation.rs`

**其他层** (15个文件):
- Bootstrap, utils, workers, 等等

### 硬编码值统计

**Worker数量**: 1处
- ✅ `src/main.rs:148` - 已配置化

**Duration硬编码**: 160处
- 优先级1（必须迁移）: ~10处
- 优先级2（建议迁移）: ~20处
- 优先级3（可选迁移）: ~130处（主要是测试）

## 下一步计划

### 短期（1-2周）

1. **迁移Domain层服务** (2天)
   - 更新4个Domain服务文件
   - 使用 `DomainError` 替换 `anyhow::Result`
   - 更新测试

2. **迁移Infrastructure层服务** (1.5天)
   - 更新5个Infrastructure文件
   - 使用 `InfrastructureError` 替换 `anyhow::Result`
   - 更新测试

3. **完成Duration配置迁移** (1天)
   - 迁移5个关键文件
   - 更新测试

### 中期（2-4周）

4. **更新其他层** (3天)
   - Workers层
   - Utils层
   - Bootstrap层

5. **文档更新** (0.5天)
   - 配置文档
   - 错误处理指南

6. **CI检查** (0.5天)
   - 添加技术债务检查
   - 防止反弹

## 成功指标

### 目标

- ✅ Worker数量可配置
- ✅ 超时配置基础设施就绪
- ⏳ anyhow使用减少50%+ (目标: <12个文件)
- ⏳ Domain层完全使用 `DomainError`
- ⏳ Infrastructure核心层使用 `InfrastructureError`
- ⏳ 配置文档完整

### 验证方法

```bash
# 检查anyhow使用
rg "anyhow::Result" --type rust | wc -l

# 检查硬编码Duration
rg "Duration::from_secs\(" --type rust | wc -l

# 运行所有测试
cargo test

# 运行clippy
cargo clippy -- -D warnings
```

## 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 迁移破坏现有功能 | 高 | 充分测试，分阶段迁移 |
| 配置过于复杂 | 低 | 提供合理默认值 |
| 错误处理过度工程 | 低 | 遵循简单优先原则 |
| 工作量超出预期 | 中 | 分批次处理，优先高价值 |

## 参考资料

- 提案文档: `openspec/changes/cleanup-code-quality-debt/proposal.md`
- 任务清单: `openspec/changes/cleanup-code-quality-debt/tasks.md`
- 设计文档: `openspec/changes/cleanup-code-quality-debt/design.md`
