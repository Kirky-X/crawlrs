# crawlrs 快速开始指南

5分钟内学会使用 crawlrs 进行网页数据采集。

## 前置条件检查

在开始之前，请确保已安装以下软件：

- **Rust 1.97+**：`rustc --version`（与主项目 `Cargo.toml` 的 `rust-version` 一致）
- **PostgreSQL 16+** 或 **SQLite 3.35+**

安装依赖：

```bash
# 安装 Rust（如果尚未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 验证安装
rustc --version
cargo --version
```

## 步骤1：创建项目

```bash
# 创建新的Rust项目
cargo new my-crawler
cd my-crawler

# 添加 crawlrs 依赖
cargo add crawlrs
```

或使用现有项目，添加依赖到 `Cargo.toml`：

```toml
[dependencies]
crawlrs = "0.2"
tokio = { version = "1.53", features = ["rt-multi-thread", "macros"] }
```

## 步骤2：编写你的第一个爬虫

创建 `src/main.rs`。库模式的唯一抓取入口是 `EngineClient`：

```rust
use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("开始爬取示例...");

    // 创建引擎客户端并发起爬取
    let client = EngineClient::new();
    let request = ScrapeRequest::new("https://example.com").timeout(Duration::from_secs(30));
    let response = client.scrape(&request).await?;

    println!("状态码: {}", response.status_code);
    println!("内容长度: {} 字符", response.content.len());

    Ok(())
}
```

## 步骤3：运行你的爬虫

```bash
cargo run
```

你应该能看到类似输出：

```
开始爬取示例...
状态码: 200
内容长度: 1256 字符
```

## 步骤4：尝试更多功能

### 渲染、截图与更多请求选项

```rust
// 启用 JavaScript 渲染（需 engine-playwright feature，路由自动改派浏览器引擎）
let result = client
    .scrape(&ScrapeRequest::new("https://example.com").needs_js())
    .await?;

// 页面截图
let result = client
    .scrape(&ScrapeRequest::new("https://example.com").needs_screenshot())
    .await?;

// 移动端 User-Agent
let result = client
    .scrape(&ScrapeRequest::new("https://example.com").mobile())
    .await?;
```

### Markdown / 结构化提取

`EngineClient` 返回引擎原始内容（`response.content` + `content_type`）。
Markdown / JSON 格式转换与 CSS / LLM 提取属于 HTTP API 能力，通过
`POST /v1/scrape` 的 `formats`、`extraction_rules`、`extraction_prompt`
参数使用，详见 [API_REFERENCE.md](../docs/API_REFERENCE.md) 与
`examples/scrape/extraction_prompt.rs` 示例。

## 步骤5：运行项目示例

项目包含丰富的示例代码（示例为根 workspace 成员 crate，需在 `examples/` 目录下以 `--bin` 运行）：

```bash
cd examples

# 运行基本抓取示例
cargo run --bin basic_scrape

# 运行搜索示例（需真实搜索引擎 / API Key）
cargo run --bin test_unified_search

# 运行浏览器示例
cargo run --bin test_playwright_basic

# 编译所有示例目标
cargo build --all-targets
```

## 常见问题

### Q: 爬取失败怎么办？

A: 检查以下内容：
- URL是否可访问
- 是否需要代理
- 是否需要JavaScript渲染
- 检查网络连接

### Q: 如何提高爬取速度？

A: 尝试以下方法：
- 使用并发爬取
- 启用缓存
- 配置适当的延迟
- 使用更快的引擎（reqwest > playwright）

### Q: 如何处理反爬措施？

A: 参考以下策略：
- 使用代理轮换
- 启用TLS指纹对抗
- 使用Playwright模拟真实浏览器
- 配置User-Agent

### Q: 如何配置代理？

```rust
use crawlrs::engines::engine_client::{EngineClient, ScrapeOptions, ScrapeRequest};

let client = EngineClient::new();
let options = ScrapeOptions {
    proxy: Some("http://proxy.example.com:8080".to_string()),
    ..Default::default()
};
let request = ScrapeRequest::new("https://example.com").with_options(options);
let result = client.scrape(&request).await?;
```

### Q: 如何使用认证？

认证属于 HTTP API 层：API Key 由 garrison 签发，通过 `Authorization` 头传递
（库内 `EngineClient` 不涉及认证）：

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer <garrison_key_id>.<garrison_secret>" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'
```

API Key 的签发与管理见 [USER_GUIDE.md](../docs/USER_GUIDE.md)。

## 下一步

完成本快速开始后，建议：

1. **阅读示例**：浏览 `examples/` 目录下的示例代码
2. **API文档**：查看 [API_REFERENCE.md](../docs/API_REFERENCE.md)
3. **用户指南**：阅读 [USER_GUIDE.md](../docs/USER_GUIDE.md)
4. **架构文档**：了解系统架构 [ARCHITECTURE.md](../docs/ARCHITECTURE.md)

## 相关资源

- [项目首页](https://github.com/Kirky-X/crawlrs)
- [完整文档](../docs/)
- [API参考](../docs/API_REFERENCE.md)
- [用户指南](../docs/USER_GUIDE.md)
- [贡献指南](../CONTRIBUTING.md)

## 获得帮助

- 📧 邮箱：Kirky-X@outlook.com
- 🐛 问题报告：[GitHub Issues](https://github.com/Kirky-X/crawlrs/issues)
- 💬 社区：[Discord](https://discord.gg/your-server)

---

**恭喜！你已经掌握了 crawlrs 的基本用法。开始你的数据采集之旅吧！** 🚀
