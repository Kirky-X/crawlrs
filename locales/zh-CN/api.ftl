# API 响应消息 — zh-CN
# 覆盖所有 handler 硬编码字符串（12 个 key）

api-access-denied = 访问被拒绝。
api-internal-error = 内部服务器错误。
api-task-not-found = 任务未找到。
api-crawl-not-found = 爬取任务未找到。
api-scrape-cancelled = 抓取任务已取消。
api-task-failed = 任务失败。
api-validation-error = 验证错误：{ $message }
api-geographic-denied = 由于地理限制，访问被拒绝。
api-webhook-url-invalid = 无效的 Webhook URL：检测到潜在安全风险。
api-insufficient-permissions-api-key = 权限不足，无法查询其他 API 密钥的审计日志。
api-insufficient-permissions-team = 权限不足，无法查询其他团队的审计日志。
api-task-ids-empty = 任务 ID 不能为空。

# 团队地理限制消息（1 个 key）
team-invalid-ip-cidr = 无效的 IP 地址或 CIDR 格式：{ $value }

# sitemap 请求源与 map 端点消息（4 个 key）
api-sitemap-feature-required = sitemap 请求源需要 `sitemap` 特性（请使用 --features sitemap 重新构建）
api-sitemap-source-disabled = sitemap 请求源已禁用（CRAWLRS__SITEMAP__ENABLED=false）；请设置 CRAWLRS__SITEMAP__ENABLED=true 以接受 sitemap_seed
api-map-sitemap-status = sitemap 返回状态码 { $status }
api-map-origin-unparsable = 无法从 { $url } 解析出 origin
