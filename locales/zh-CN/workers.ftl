# Worker 运维日志消息 — zh-CN
# 覆盖 BacklogWorker::process_backlog() 日志语句（5 个 key）

backlog-processing-started = 开始处理积压任务
backlog-none-pending = 没有待处理的积压任务
backlog-pending-found = 发现 { $count } 个待处理的积压任务
backlog-team-processing = 处理团队 { $team_id } 的 { $count } 个积压任务
backlog-processing-summary = 积压任务处理完成：成功={ $reactivated }，过期={ $expired }，重试耗尽={ $retry_exhausted }，并发拒绝={ $denied }，意外排队={ $queued }，跳过={ $skipped }，错误={ $failed }

# 积压 Worker 详细日志（20 keys）
backlog-processing-failed = 处理积压任务失败: { $error }
backlog-expired-marked = 积压任务 { $task_id } 已过期，标记为过期状态
backlog-retry-exhausted-marked = 积压任务 { $task_id } 重试次数已达上限，标记为失败
backlog-concurrency-available = 团队 { $team_id } 并发槽位可用，处理积压任务 { $task_id }
backlog-state-transition-skipped = 积压任务 { $task_id } 无法迁移到 Processing（当前状态 { $current }）: { $error }，跳过
backlog-reactivated = 积压任务 { $task_id } 重新激活成功
backlog-reactivation-failed = 重新激活任务失败: { $error }
backlog-concurrency-denied = 团队 { $team_id } 并发限制未释放: { $reason }，积压任务 { $task_id } 继续保持积压状态
backlog-unexpected-requeue = 积压任务 { $task_id } 被重新排队，这是意外的行为
backlog-concurrency-check-failed = 检查团队并发限制失败: { $error }
backlog-task-status-not-activatable = 任务 { $task_id } 状态为 { $status }，不需要重新激活
backlog-task-reactivated = 任务 { $task_id } 重新激活成功
backlog-cleanup-started = 开始清理过期积压任务
backlog-cleanup-none = 没有过期的积压任务
backlog-cleanup-failed = 清理过期积压任务失败: { $error }
backlog-cleanup-completed = 清理过期积压任务完成，共清理 { $count } 个任务
backlog-cleanup-processing = 处理过期积压任务 { $task_id }
backlog-task-marked-failed-by-expiry = 任务 { $task_id } 因积压过期被标记为失败
backlog-process-error = 处理积压任务时发生错误: { $error }
backlog-cleanup-error = 清理过期积压任务时发生错误: { $error }

# 链接提取器日志（2 keys）
link-score-skip = task_id: { $task_id }，URL 评分失败跳过: { $error }（{ $url }）
link-frontier-enqueued = task_id: { $task_id }，{ $count } 个 URL 入 Frontier（{ $domains } 个域名），按评分出队

# 抓取执行器日志（5 keys）
scrape-exec-encoding-started = 开始处理文本编码转换，任务ID: { $task_id }，URL: { $url }
scrape-exec-encoding-succeeded = 文本编码处理成功，检测到的编码: { $encoding }，处理时间: { $elapsed }ms，质量评分: { $quality }
scrape-exec-encoding-failed = 文本编码处理失败: { $error }
scrape-exec-encoding-error = 文本编码处理异常: { $error }
scrape-task-encoding-fallback = 文本编码处理失败，使用原始内容: { $error }
