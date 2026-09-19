# Worker 运维日志消息 — zh-CN
# 覆盖 BacklogWorker::process_backlog() 日志语句（5 个 key）

backlog-processing-started = 开始处理积压任务
backlog-none-pending = 没有待处理的积压任务
backlog-pending-found = 发现 { $count } 个待处理的积压任务
backlog-team-processing = 处理团队 { $team_id } 的 { $count } 个积压任务
backlog-processing-summary = 积压任务处理完成：成功={ $reactivated }，过期={ $expired }，重试耗尽={ $retry_exhausted }，并发拒绝={ $denied }，意外排队={ $queued }，跳过={ $skipped }，错误={ $failed }
