# 优化测试快速开始指南

## 快速开始

### 1. 运行所有优化测试

```bash
./run_optimized_tests.sh
```

### 2. 运行网页采集测试

```bash
./run_optimized_tests.sh scrape
```

### 3. 运行搜索引擎测试

```bash
./run_optimized_tests.sh search
```

### 4. 运行综合测试

```bash
./run_optimized_tests.sh combined
```

### 5. 运行特定测试（带详细输出）

```bash
./run_optimized_tests.sh -v test_random_news_scrape
```

## 测试说明

### 网页采集测试
- 随机选择 5 个新闻网站中的一个
- 从该网站的 2 个页面中随机选择一个进行采集
- 验证采集结果（状态码、内容长度、HTML 标签）

### 搜索引擎测试
- 随机选择 10 个关键词中的一个
- 使用所有搜索引擎进行搜索
- 至少需要 1 个引擎成功通过测试

## 常用命令

```bash
# 查看帮助
./run_optimized_tests.sh --help

# 运行所有测试（详细输出）
./run_optimized_tests.sh -v

# 使用 Cargo 命令运行
cargo test --test integration_tests optimized_tests -- --nocapture

# 运行特定测试
cargo test --test integration_tests -- test_random_news_scrape -- --nocapture
```

## 注意事项

1. 需要真实的网络连接
2. 测试之间有延迟，避免被反爬虫
3. 某些搜索引擎可能需要 API 密钥

## 更多信息

- [详细说明](./optimized_tests_README.md)
- [优化总结](./TEST_OPTIMIZATION_SUMMARY.md)