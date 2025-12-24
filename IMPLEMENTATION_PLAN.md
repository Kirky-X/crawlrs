# 实施计划：统一搜索引擎使用智能路由

## 目标
将所有搜索引擎（Google、Bing、Baidu）统一通过 `EngineRouter` 进行路由，实现 PRD 中定义的智能路由功能。

## 当前状态

| 搜索引擎 | 当前实现 | 使用 EngineRouter |
|---------|---------|------------------|
| SmartSearchEngine | 封装 EngineRouter | ✅ 是 |
| GoogleSearchEngine | 直接使用 Playwright + HTTP fallback | ❌ 否 |
| BingSearchEngine | 直接使用 reqwest::Client | ❌ 否 |
| BaiduSearchEngine | 直接使用 reqwest::Client | ❌ 否 |

## 重构方案

### 方案：创建统一的 SmartSearchEngine 变体

参考 `SmartSearchEngine` 的实现，为每个搜索引擎创建基于 EngineRouter 的封装：

1. **GoogleSmartSearchEngine** - 使用 `EngineRouter`，需要 JS 支持
2. **BingSmartSearchEngine** - 使用 `EngineRouter`，可选 JS
3. **BaiduSmartSearchEngine** - 使用 `EngineRouter`，可选 JS

### 关键变化

1. **移除直接 HTTP 客户端依赖** - 统一通过 EngineRouter 路由
2. **保留 URL 构建逻辑** - 每个引擎保留其特定的 URL 构建逻辑
3. **保留结果解析逻辑** - 保留每个搜索引擎的 HTML 解析逻辑
4. **统一工厂函数** - 在 `mod.rs` 中导出统一的创建函数

## 实施步骤

### 步骤 1: 重构 GoogleSearchEngine
- [ ] 添加 EngineRouter 依赖
- [ ] 重构 search 方法使用 router.route()
- [ ] 保留 URL 构建逻辑
- [ ] 保留测试数据加载逻辑

### 步骤 2: 重构 BingSearchEngine
- [ ] 添加 EngineRouter 依赖
- [ ] 重构 search 方法使用 router.route()
- [ ] 保留 URL 构建逻辑
- [ ] 保留测试数据加载逻辑

### 步骤 3: 重构 BaiduSearchEngine
- [ ] 添加 EngineRouter 依赖
- [ ] 重构 search 方法使用 router.route()
- [ ] 保留 URL 构建逻辑
- [ ] 保留测试数据加载逻辑

### 步骤 4: 更新 mod.rs
- [ ] 导出统一的工厂函数
- [ ] 添加 EngineRouter 初始化示例

### 步骤 5: 测试验证
- [ ] 运行单元测试
- [ ] 运行集成测试
- [ ] 验证编译通过

## 技术细节

### 引擎选择策略

| 搜索引擎 | needs_js | needs_screenshot | needs_tls_fingerprint |
|---------|----------|-----------------|---------------------|
| Google | true | false | false |
| Bing | false | false | false |
| Baidu | false | false | false |

### ScrapeRequest 配置示例

```rust
ScrapeRequest {
    url: search_url,
    headers: HashMap::new(),
    timeout: Duration::from_secs(30),
    needs_js: true,        // Google 需要 JS
    needs_screenshot: false,
    screenshot_config: None,
    mobile: false,
    proxy: None,
    skip_tls_verification: false,
    needs_tls_fingerprint: false,
    use_fire_engine: false,
    actions: Vec::new(),
    sync_wait_ms: 0,
}
```

## 风险与缓解

| 风险 | 缓解措施 |
|-----|---------|
| 现有功能退化 | 保留原有实现作为备选，测试验证 |
| 性能下降 | EngineRouter 已有性能优化机制 |
| 测试数据失效 | 保留测试数据加载逻辑 |

## 验收标准

1. 所有搜索引擎通过 EngineRouter 进行路由
2. 保留原有功能（URL 构建、结果解析、测试数据）
3. 编译通过，所有测试通过
4. 性能不低于原有实现
