# 错误消息 — zh-CN
# 覆盖所有 CrawlRsError::user_message() 变体（17 个 key）

error-database = 数据库操作失败，请稍后重试。
error-network = 外部服务不可用，请稍后重试。
error-config = 配置错误，请联系技术支持。
error-validation = 验证错误：{ $message }
error-not-found = 资源未找到：{ $resource }
error-auth = 认证失败：{ $reason }
error-permission = 权限不足。
error-timeout = 请求超时，请稍后重试。
error-rate-limit = 请求频率超限，请降低请求速度。
error-quota = 配额已用尽，请升级您的方案。
error-service-unavailable = 服务不可用，请稍后重试。
error-cache = 缓存服务不可用，请稍后重试。
error-task = 任务处理错误，请稍后重试。
error-json = JSON 格式无效，请检查您的请求。
error-io = 内部 I/O 错误，请稍后重试。
error-engine = 引擎错误。
error-internal = 内部服务器错误，请稍后重试。

# 领域错误消息（17 个 key）

domain-error-crawl-config = 爬虫配置错误：{ $message }
domain-error-depth-exceeded = 爬取深度超出限制：最大深度 { $max }，请求深度 { $requested }。
domain-error-path-filtered = URL 路径被过滤规则排除：{ $path }
domain-error-task-not-found = 任务未找到：{ $task_id }
domain-error-invalid-task-state = 任务状态无效：当前状态 { $current }，期望状态 { $expected }。
domain-error-task-expired = 任务已过期。
domain-error-team-not-found = 团队不存在：{ $team_id }
domain-error-insufficient-credits = 积分不足：需要 { $required }，可用 { $available }。
domain-error-concurrency-limit = 团队并发限制：当前 { $current }，限制 { $limit }。
domain-error-invalid-url = 无效的 URL：{ $url }
domain-error-domain-blacklisted = URL 在黑名单中：{ $domain }
domain-error-robots-forbidden = URL 被 robots.txt 禁止：{ $url }
domain-error-webhook-delivery-failed = Webhook 投递失败。
domain-error-invalid-webhook-url = 无效的 Webhook URL：{ $url }
domain-error-llm-extraction-failed = LLM 提取失败。
domain-error-invalid-css-selector = CSS 选择器无效：{ $selector }
domain-error-validation = 验证错误：{ $field } - { $message }
