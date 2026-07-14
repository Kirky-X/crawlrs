// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 结果分页示例
//!
//! 演示如何获取和分页浏览爬取结果。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example result_pagination
//!

use log::info;

/// 分页查询参数
#[derive(Debug, Clone)]
struct PaginationParams {
    page: u32,
    page_size: u32,
    sort_by: String,
    sort_order: String,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: 20,
            sort_by: "created_at".to_string(),
            sort_order: "desc".to_string(),
        }
    }
}

/// 分页结果
#[derive(Debug)]
struct PaginatedResult<T> {
    items: Vec<T>,
    total: u32,
    page: u32,
    #[allow(dead_code)]
    page_size: u32,
    total_pages: u32,
    has_next: bool,
    has_previous: bool,
}

impl<T> PaginatedResult<T> {
    fn new(items: Vec<T>, total: u32, page: u32, page_size: u32) -> Self {
        let total_pages = (total as f64 / page_size as f64).ceil() as u32;
        Self {
            items,
            total,
            page,
            page_size,
            total_pages,
            has_next: page < total_pages,
            has_previous: page > 1,
        }
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始结果分页示例");
    info!("=====================================\n");

    // 1. 分页参数配置
    info!("1️⃣  分页参数配置");
    info!("-----------------------------");

    let params = PaginationParams {
        page: 1,
        page_size: 20,
        sort_by: "created_at".to_string(),
        sort_order: "desc".to_string(),
    };

    info!("📋 分页参数:");
    info!("   页码: {}", params.page);
    info!("   每页大小: {}", params.page_size);
    info!("   排序字段: {}", params.sort_by);
    info!("   排序方向: {}", params.sort_order);
    info!("");

    // 2. 分页计算
    info!("2️⃣  分页计算");
    info!("-----------------------------");

    let total_items: u32 = 150;
    let page_size: u32 = 20;
    let total_pages = (total_items as f64 / page_size as f64).ceil() as u32;

    info!("📊 总数据量: {} 条", total_items);
    info!("   每页大小: {} 条", page_size);
    info!("   总页数: {} 页", total_pages);
    info!("");

    info!("📋 各页数据范围:");
    for page in 1..=total_pages.min(8) {
        let start = (page - 1) * page_size + 1;
        let end = (page * page_size).min(total_items);
        info!("   第{:2}页: {:3} - {:3}", page, start, end);
    }
    info!("");

    // 3. 实际分页查询示例
    info!("3️⃣  分页查询示例");
    info!("-----------------------------");

    // 模拟生成分页结果
    let mock_results: Vec<String> = (1..=20).map(|i| format!("结果项 #{}", i)).collect();
    let result = PaginatedResult::new(mock_results, total_items, 1, page_size);

    info!("📝 第1页结果:");
    info!("   显示: {} 条", result.items.len());
    info!("   总数: {} 条", result.total);
    info!("   页码: {}/{}", result.page, result.total_pages);
    info!("   是否有下一页: {}", result.has_next);
    info!("   是否有上一页: {}", result.has_previous);
    info!("");

    // 4. 分页导航示例
    info!("4️⃣  分页导航");
    info!("-----------------------------");

    info!("📋 生成分页链接:");
    let base_url = "/api/v1/crawl/results";
    let query_params = "sort=created_at&order=desc";

    for page in 1..=5 {
        let url = format!("{}?{}&page={}&size=20", base_url, query_params, page);
        let is_current = if page == 1 { " (当前页)" } else { "" };
        info!("   {}页: {}{}", page, url, is_current);
    }
    info!("");

    // 5. 大数据量优化
    info!("5️⃣  大数据量优化建议");
    info!("-----------------------------");
    info!("");
    info!("📖 对于大数据量的分页:");
    info!("   1. 使用游标分页代替偏移分页");
    info!("      - 避免OFFSET过大导致的性能问题");
    info!("      - 适用于实时数据场景");
    info!("");
    info!("   2. 限制最大分页大小");
    info!("      - 防止恶意请求");
    info!("      - 建议最大100-500条");
    info!("");
    info!("   3. 添加总数缓存");
    info!("      - 总数变化不频繁时可以缓存");
    info!("      - 减少数据库查询");
    info!("");
    info!("   4. 使用覆盖索引");
    info!("      - 确保分页查询能使用索引");
    info!("      - 避免回表查询");
    info!("");

    // 6. 示例：游标分页
    info!("6️⃣  游标分页示例");
    info!("-----------------------------");

    info!("📝 游标分页API:");
    info!("   GET /api/v1/crawl/results?cursor=abc123&limit=20");
    info!("");
    info!("📋 响应格式:");
    info!("   {{");
    info!("     \"items\": [...],           // 数据列表");
    info!("     \"next_cursor\": \"xyz789\", // 下一页游标（null表示无更多数据）");
    info!("     \"has_more\": true          // 是否有更多数据");
    info!("   }}");
    info!("");

    info!("💡 游标分页优势:");
    info!("   - 性能稳定，不受数据量影响");
    info!("   - 支持实时数据场景");
    info!("   - 避免分页跳过问题");

    info!("\n=====================================");
    info!("✨ 结果分页示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 根据数据量选择合适的分页方式");
    info!("   - 小数据量：偏移分页简单直观");
    info!("   - 大数据量：游标分页性能更好");
    info!("   - 始终设置合理的分页大小限制");
}
