# crawlrs 快速开始指南

5分钟内学会使用 crawlrs 进行网页数据采集。

## 前置条件检查

在开始之前，请确保已安装以下软件：

- **Rust 1.70+**：`rustc --version`
- **PostgreSQL 14+** 或 **SQLite 3.35+**

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
crawlrs = "0.1"
```

## 步骤2：编写你的第一个爬虫

创建 `src/main.rs`：

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    println!("开始爬取示例...");

    // 基本爬取
    let result = scrape("https://example.com")
        .format("markdown")
        .await?;

    println!("标题: {}", result.metadata.title);
    println!("内容长度: {} 字符", result.content.len());

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
标题: Example Domain
内容长度: 1234 字符
```

## 步骤4：尝试更多功能

### 多格式输出

```rust
// 获取HTML
let html = scrape("https://example.com")
    .format("html")
    .await?;

// 获取Markdown
let markdown = scrape("https://example.com")
    .format("markdown")
    .await?;

// 获取JSON
let json_data = scrape("https://example.com")
    .format("json")
    .await?;

// 获取截图
let screenshot = scrape("https://example.com")
    .screenshot(true)
    .await?;
```

### 使用提取规则

```rust
let result = scrape("https://example.com")?
    .extract(|extractor| {
        extractor
            .selector("h1")
            .text()
            .selector(".content p")
            .all()
    })
    .await?;
```

### 使用浏览器引擎

```rust
// 启用JavaScript渲染
let result = scrape("https://example.com")
    .engine("playwright")
    .wait_for_selector("#content")
    .await?;
```

## 步骤5：运行项目示例

项目包含丰富的示例代码：

```bash
# 运行基本爬取示例
cargo run --example basic_scrape

# 运行搜索示例
cargo run --example test_google

# 运行浏览器示例
cargo run --example test_playwright_basic

# 运行所有示例（需要full特性）
cargo run --features full --examples
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
let result = scrape("https://example.com")
    .proxy("http://proxy.example.com:8080")
    .await?;
```

### Q: 如何使用认证？

```rust
let client = crawlrs::Client::with_api_key("your-api-key");
let result = client.scrape("https://example.com").await?;
```

## 下一步

完成本快速开始后，建议：

1. **阅读示例**：浏览 `examples/` 目录下的示例代码
2. **API文档**：查看 [API_REFERENCE.md](../docs/API_REFERENCE.md)
3. **用户指南**：阅读 [USER_GUIDE.md](../docs/USER_GUIDE.md)
4. **架构文档**：了解系统架构 [ARCHITECTURE.md](../docs/ARCHITECTURE.md)

## 相关资源

- [项目首页](https://github.com/your-org/crawlrs)
- [完整文档](../docs/)
- [API参考](../docs/API_REFERENCE.md)
- [用户指南](../docs/USER_GUIDE.md)
- [贡献指南](../CONTRIBUTING.md)

## 获得帮助

- 📧 邮箱：Kirky-X@outlook.com
- 🐛 问题报告：[GitHub Issues](https://github.com/your-org/crawlrs/issues)
- 💬 社区：[Discord](https://discord.gg/your-server)

---

**恭喜！你已经掌握了 crawlrs 的基本用法。开始你的数据采集之旅吧！** 🚀
