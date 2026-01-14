# Crawlrs 测试计划总结

## 测试目标

本测试计划旨在全面验证 crawlrs 项目在模拟真实生产环境下的：
1. Docker 容器化部署能力
2. 多服务集成正确性
3. 所有 REST API 端点功能
4. 系统性能和稳定性
5. 错误处理和容错能力
6. 特性开关组合兼容性

## 测试范围

### 测试覆盖的 API 端点

#### 公共端点
- `GET /health` - 健康检查
- `GET /metrics` - Prometheus 指标
- `GET /v1/version` - 版本信息

#### 受保护端点
- `POST /v1/search` - 搜索
- `POST /v1/crawl` - 创建爬取任务
- `GET /v1/crawl/{id}` - 获取爬取状态
- `GET /v1/crawl/{id}/results` - 获取爬取结果
- `DELETE /v1/crawl/{id}` - 取消爬取任务
- `POST /v1/scrape` - 创建抓取任务
- `GET /v1/scrape/{id}` - 获取抓取状态
- `DELETE /v1/scrape/{id}` - 取消抓取任务
- `POST /v1/extract` - 数据提取
- `POST /v1/webhooks` - 创建 Webhook
- `GET /v1/teams/geo-restrictions` - 获取地理限制
- `PUT /v1/teams/geo-restrictions` - 更新地理限制
- `GET /v1/audit/logs` - 审计日志
- `GET /v1/audit/denied` - 被拒绝请求

## 测试环境

### 服务配置

| 服务 | 镜像 | 端口 | 用途 |
|------|------|------|------|
| PostgreSQL | postgres:15-alpine | 5432 | 主数据库 |
| Redis | redis:7-alpine | 6379 | 缓存和限流 |
| Chrome | browserless/chrome | 9222 | JavaScript 渲染 |
| FlareSolverr | ghcr.io/flaresolverr/flaresolverr | 8191 | 反爬虫解决方案 |
| MinIO | minio/minio | 9000 | 对象存储 |
| Crawlrs | 自定义 | 3000 | 主应用 API |
| Prometheus | prom/prometheus | 9090 | 监控指标 |
| Grafana | grafana/grafana | 3001 | 可视化 |

### 测试配置矩阵

#### 配置 1: MinIO Only
- 测试 MinIO 存储功能
- 禁用浏览器和搜索功能

#### 配置 2: Browser Only
- 测试 Chrome 和 FlareSolverr
- 禁用存储和搜索功能

#### 配置 3: Search Only
- 测试搜索引擎集成
- 禁用浏览器和存储功能

#### 配置 4: Full Features
- 启用所有功能
- 完整的端到端测试

## 测试套件

### 1. API 端点测试
- 健康检查端点
- 搜索端点
- 爬取端点
- 抓取端点
- 提取端点
- Webhook 端点
- 团队管理端点
- 审计端点

### 2. 性能测试
- P99 延迟测试 (< 200ms)
- P95 延迟测试 (< 150ms)
- 平均响应时间 (< 100ms)
- 每秒请求数 (RPS)
- 并发请求处理
- 缓存命中率

### 3. 错误处理测试
- 错误响应验证
- 服务韧性测试
- 数据完整性测试
- 安全测试 (SQL 注入, XSS)
- 速率限制测试

### 4. 特性组合测试
- MinIO 存储测试
- 浏览器渲染测试
- 搜索引擎测试
- 完整功能测试

## 性能指标要求

| 指标 | 目标值 | 测试方法 |
|------|--------|---------|
| API 吞吐量 | 10000 RPS | 并发负载测试 |
| P50 延迟 | < 50ms | 延迟分布测试 |
| P99 延迟 | < 200ms | 延迟分布测试 |
| 成功率 | > 99.9% | 错误率监控 |
| 缓存命中率 | > 60% | Redis 缓存测试 |

## 测试输出

### 测试报告
- JSON 格式: `test-results/report.json`
- HTML 格式: `test-results/report.html`
- 包含详细请求/响应信息

### 监控面板
- Prometheus: 实时指标
- Grafana: 可视化监控

## 风险和缓解措施

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| 外部服务不可用 | 测试失败 | 使用 Mock 服务 |
| 网络延迟高 | 性能测试不准确 | 本地 Docker 环境 |
| 资源不足 | 测试超时 | 充足的硬件配置 |
| 数据污染 | 测试结果不可靠 | 环境隔离和清理 |

## 执行计划

### 阶段 1: 环境准备
1. 创建 Docker Compose 配置
2. 设置测试环境变量
3. 验证服务健康状态

### 阶段 2: API 测试
1. 执行 API 端点测试
2. 验证请求/响应格式
3. 记录测试结果

### 阶段 3: 性能测试
1. 执行延迟测试
2. 执行吞吐量测试
3. 执行并发测试

### 阶段 4: 特性测试
1. 执行 MinIO 测试
2. 执行浏览器测试
3. 执行搜索测试
4. 执行完整功能测试

### 阶段 5: 报告生成
1. 汇总测试结果
2. 生成测试报告
3. 分析性能指标

## 验收标准

### 必须通过
- [ ] 所有 API 端点响应正确
- [ ] P99 延迟 < 200ms
- [ ] 测试成功率 > 99%
- [ ] 错误处理正确
- [ ] 数据完整性验证通过

### 加分项
- [ ] P99 延迟 < 100ms
- [ ] 测试成功率 = 100%
- [ ] 缓存命中率 > 70%
- [ ] RPS > 1000
