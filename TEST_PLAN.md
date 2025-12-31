# 测试和修复计划

## 目标
运行全部测试套件（tests/ 和 examples/），使用 Docker 部署相关组件，分析并修复所有失败的测试，确保测试通过。

## 测试范围

### 1. 单元测试 (tests/unit/)
- config/ - 配置单元测试
- domain/ - 领域层测试（models, services）
- engines/ - 引擎测试（Reqwest, Playwright, FireEngine）
- infrastructure/ - 基础设施测试
- presentation/ - 表示层测试（handlers, middleware）
- workers/ - 工作器测试
- utils/ - 工具函数测试

### 2. 集成测试 (tests/integration/)
- api_tests.rs - API 端点测试
- health_check.rs - 健康检查测试
- scrape_test.rs - 抓取功能测试
- search_engines_test.rs - 搜索引擎测试
- browser_tests.rs - 浏览器测试
- scheduler_test.rs - 调度器测试
- webhook_test.rs - Webhook 测试
- repositories/ - 仓储测试
- helpers/ - 测试辅助工具

### 3. 端到端测试 (tests/e2e/)
- complete_workflow_test.rs - 完整工作流测试
- user_journey_test.rs - 用户旅程测试
- business_scenarios_test.rs - 业务场景测试
- performance_workflow_test.rs - 性能工作流测试
- test_scenarios.py - Python 端到端测试

### 4. 示例代码 (examples/)
- browser/engine_router_demo.rs - 引擎路由演示
- search/test_smart_search.rs - 智能搜索演示
- search/test_*.rs - 其他搜索示例
- text_encoding/* - 文本编码示例

## Docker 环境配置

### 服务组件（使用 docker-compose.test.yml）
- **PostgreSQL**: 端口 5433，数据库 crawlrs_test
- **Redis**: 端口 6381
- **Chrome**: 端口 9222，用于浏览器测试
- **FlareSolverr**: 端口 8191，用于 Google 搜索代理

### 环境变量配置
```bash
# 数据库
DB_HOST=localhost
DB_PORT=5433
DB_USER=crawlrs
DB_PASSWORD=password
DB_NAME=crawlrs_test

# Redis
REDIS_HOST=localhost
REDIS_PORT=6381

# 浏览器
CHROMIUM_REMOTE_DEBUGGING_URL=http://localhost:9222
CHROME_HOST=localhost
CHROME_PORT=9222

# 搜索引擎
FLARESOLVERR_HOST=localhost
FLARESOLVERR_PORT=8191

# 测试模式
CRAWLRS_DISABLE_SSRF_PROTECTION=true
USE_TEST_DATA=1
GOOGLE_HTTP_FALLBACK_TEST_RESULTS=true
BING_TEST_RESULTS=true
BAIDU_TEST_RESULTS=true
SOGOU_TEST_RESULTS=true
```

## 执行步骤

### 阶段 1：环境准备
1. 清理旧的 Docker 容器和卷
2. 启动测试环境 Docker 服务
3. 验证所有服务健康状态
4. 运行数据库迁移
5. 初始化测试数据

### 阶段 2：单元测试
1. 运行所有单元测试：`cargo test --lib`
2. 收集失败的测试
3. 分析失败原因
4. 修复失败的单元测试
5. 重新运行验证

### 阶段 3：集成测试
1. 运行集成测试：`cargo test --test integration_tests`
2. 包含被忽略的测试：`-- --include-ignored`
3. 收集失败的测试
4. 分析失败原因（环境、配置、代码）
5. 修复失败的集成测试
6. 重新运行验证

### 阶段 4：端到端测试
1. 运行 E2E Rust 测试：`cargo test --test e2e_tests`
2. 运行 Python E2E 测试：`python tests/e2e/test_scenarios.py`
3. 收集失败的测试
4. 分析失败原因
5. 修复失败的端到端测试
6. 重新运行验证

### 阶段 5：示例代码测试
1. 运行引擎路由演示：`cargo run --example engine_router_demo`
2. 运行智能搜索演示：`cargo run --example test_smart_search`
3. 运行其他搜索示例
4. 运行文本编码示例
5. 分析运行错误
6. 修复示例代码问题

### 阶段 6：最终验证
1. 运行完整测试套件：`cargo test --all -- --include-ignored`
2. 生成测试报告
3. 确认所有测试通过
4. 清理 Docker 环境

## 预期问题和解决方案

### 常见问题 1：数据库连接失败
**原因**: Docker 服务未启动或端口冲突
**解决**:
- 检查 Docker 容器状态：`docker-compose -f docker-compose.test.yml ps`
- 重新启动服务：`docker-compose -f docker-compose.test.yml restart`
- 验证数据库连接：`psql postgres://crawlrs:password@localhost:5433/crawlrs_test`

### 常见问题 2：Redis 连接失败
**原因**: Redis 服务未启动或端口冲突
**解决**:
- 检查 Redis 容器：`docker ps | grep redis`
- 测试连接：`redis-cli -h localhost -p 6381 ping`
- 重启 Redis：`docker-compose -f docker-compose.test.yml restart redis`

### 常见问题 3：浏览器测试失败
**原因**: Chrome 未启动或 CDP 连接失败
**解决**:
- 检查 Chrome 容器：`docker ps | grep chrome`
- 测试 CDP：`curl http://localhost:9222/json/version`
- 增加 Chrome 超时时间
- 修复浏览器连接配置

### 常见问题 4：搜索引擎测试超时
**原因**: 网络问题或 FlareSolverr 未启动
**解决**:
- 启用测试数据模式：`USE_TEST_DATA=1`
- 检查 FlareSolverr：`curl http://localhost:8191/health`
- 增加超时时间配置
- 使用 Mock 数据替代真实搜索

### 常见问题 5：Worker 测试失败
**原因**: Worker 进程未运行
**解决**:
- 启动 Worker 进程：`cargo run -- worker`
- 或者修改测试使用 Mock Worker
- 检查任务队列连接

## 关键文件列表

### 测试配置文件
- `/home/dev/crawlrs/config/default.toml` - 应用配置
- `/home/dev/crawlrs/docker/docker-compose.test.yml` - 测试环境配置
- `/home/dev/crawlrs/docker/.env.example` - 环境变量模板

### 测试入口文件
- `/home/dev/crawlrs/tests/main.rs` - 测试主入口
- `/home/dev/crawlrs/tests/integration/mod.rs` - 集成测试模块
- `/home/dev/crawlrs/tests/e2e/mod.rs` - E2E 测试模块

### 测试辅助文件
- `/home/dev/crawlrs/tests/integration/helpers/test_app.rs` - 测试应用工厂
- `/home/dev/crawlrs/tests/integration/helpers/mod.rs` - 测试辅助工具

### 示例代码
- `/home/dev/crawlrs/examples/mod.rs` - 示例模块定义
- `/home/dev/crawlrs/examples/browser/engine_router_demo.rs` - 引擎路由演示
- `/home/dev/crawlrs/examples/search/test_smart_search.rs` - 智能搜索演示

## 成功标准

1. ✅ 所有单元测试通过（0 失败）
2. ✅ 所有集成测试通过（0 失败）
3. ✅ 所有端到端测试通过（0 失败）
4. ✅ 所有示例代码成功运行
5. ✅ 测试覆盖率报告生成
6. ✅ 测试环境清理完成

## 注意事项

1. **测试隔离**: 每个测试使用独立的数据库和 Redis 连接
2. **超时设置**: 集成测试超时 60 秒，E2E 测试超时 120 秒
3. **资源清理**: 测试后自动清理临时数据
4. **日志记录**: 使用 `RUST_LOG=debug` 获取详细日志
5. **错误处理**: 修复代码时保持向后兼容性