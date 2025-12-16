# crawlrs 文档与代码交叉验证报告

本报告对 `USER_GUIDE.md` 文档与实际代码实现进行了全面的交叉验证检查。以下是发现的差异和问题。

## 1. 概览

- **文档版本**: 2024-12-10
- **代码版本**: 0.1.0
- **检查范围**: `src` 目录（核心逻辑）、`migration` 目录（数据库结构）、`Cargo.toml`（配置）

## 2. 差异报告

### 2.1 提取 (Extract) 功能 API 缺失 [关键]

**描述**: 文档中详细描述了 `/v1/extract` 接口，支持基于 Prompt 和 Schema 的提取，以及 LLM 模型选择。但在代码的路由定义 `src/presentation/routes/mod.rs` 中，完全没有发现 `/v1/extract` 端点。虽然代码中存在 `LLMService` 和 `ExtractionService`，但它们似乎只作为内部服务被抓取/爬取功能调用，或者尚未通过 API 暴露。

**文档**:
```bash
curl -X POST https://api.crawlrs.com/v1/extract ...
```

**代码 (`src/presentation/routes/mod.rs`)**:
- 仅有 `/v1/scrape`, `/v1/crawl`, `/v1/search`, `/v1/webhooks` 等。
- 缺失 `/v1/extract`。

**修正建议**:
- 如果 `/v1/extract` 是计划中的功能，请尽快实现对应的 Handler 和 Route。
- 如果该功能已合并到 `scrape` 或 `crawl` 中，请更新文档以反映实际用法（例如通过 `extraction_rules` 参数）。

### 2.2 版本号不一致 [一般]

**描述**: 文档末尾注明 "最后更新: 2024-12-10"，且示例中多次出现 `crawlrs 0.1.0` 或类似版本暗示。`Cargo.toml` 中版本确实是 `0.1.0`。但 `migration` 目录下的迁移文件时间戳为 `20251211`（未来时间？或仅仅是命名约定？），而文档更新时间为 2024 年。这可能导致用户对版本进度的困惑。

**修正建议**:
- 统一时间线。如果项目处于 2025 年开发周期，请更新文档日期。

### 2.3 抓取 (Scrape) API 参数差异 [一般]

**描述**:
1. **`actions` 参数**: 文档提到 `/v1/scrape` 支持 `actions` 数组（如 `click`, `scroll`, `wait`）。但在 `ScrapeRequestDto` (`src/application/dto/scrape_request.rs`) 中，并没有发现 `actions` 字段。代码中只有 `wait_for` (u64) 这种简单的等待参数。
2. **`options` 结构**: 文档示例将 `headers`, `timeout`, `mobile` 放在 `options` 对象中。但 `ScrapeRequestDto` 直接将这些字段放在根对象中（如 `pub headers: Option<Value>`, `pub timeout: Option<u64>`）。这会导致用户按照文档请求时解析失败。

**文档**:
```json
{
  "url": "...",
  "options": {
    "timeout": 30,
    "mobile": false
  }
}
```

**代码 (`ScrapeRequestDto`)**:
```rust
pub struct ScrapeRequestDto {
    pub url: String,
    pub timeout: Option<u64>,
    pub mobile: Option<bool>,
    // ... no "options" field wrapping these
}
```

**修正建议**:
- **修改代码**: 调整 DTO 以匹配文档的嵌套结构（推荐，保持 API 整洁）。
- **或修改文档**: 更新文档示例以反映扁平化的 JSON 结构。
- **添加 `actions`**: 如果需要支持交互操作，需在 DTO 中添加 `actions` 字段并在 `PlaywrightEngine` 中实现对应逻辑。

### 2.4 爬取 (Crawl) API 返回字段差异 [一般]

**描述**: 文档中 `POST /v1/crawl` 的响应包含 `expires_at` 字段。但在 `Crawl` 实体 (`src/domain/models/crawl.rs`) 和相关 DTO 中，未发现 `expires_at` 字段。

**修正建议**:
- 确认业务逻辑是否需要过期时间。如果不需要，从文档中移除该字段说明。

### 2.5 搜索 (Search) API 参数差异 [一般]

**描述**: 文档中搜索接口参数为 `sources` (数组)，示例为 `["web"]`。但在 `SearchRequestDto` (`src/application/dto/search_request.rs`) 中，对应字段是 `engine: Option<String>`，且没有 `sources` 字段。

**文档**:
```json
{
  "query": "...",
  "sources": ["web"]
}
```

**代码**:
```rust
pub struct SearchRequestDto {
    pub query: String,
    pub engine: Option<String>,
    // ...
}
```

**修正建议**:
- 统一参数命名。建议代码改为 `engines` 或文档改为 `engine`。

## 3. 问题分类汇总

| ID | 问题描述 | 严重程度 | 涉及模块 | 建议操作 |
|----|----------|----------|----------|----------|
| 1 | 缺失 `/v1/extract` API 实现 | **关键** | API/Routes | 实现 API 或修改文档 |
| 2 | `/v1/scrape` 参数结构不匹配 (`options` vs flat) | **重要** | API/DTO | 调整 DTO 结构以匹配文档 |
| 3 | `/v1/scrape` 缺失 `actions` 交互功能支持 | **重要** | API/Engine | 实现页面交互逻辑或移除文档说明 |
| 4 | `/v1/search` 参数名不匹配 (`sources` vs `engine`) | **一般** | API/DTO | 统一参数名称 |
| 5 | 文档与代码时间戳/版本细微不一致 | **低** | Meta | 更新文档日期 |

## 4. 结论

`crawlrs` 的核心功能（抓取、爬取、搜索）在代码中均有基础实现，但 **API 契约（JSON 结构）与文档存在显著差异**。最严重的问题是文档承诺的独立提取 API (`/v1/extract`) 尚未公开，以及 `scrape` 接口的参数结构不一致，这将直接导致用户请求失败。

建议优先修复 DTO 结构以匹配文档，并补全缺失的提取 API。
