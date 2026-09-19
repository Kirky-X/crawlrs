# Worker operational log messages — en-US
# Covers BacklogWorker::process_backlog() log statements (5 keys)

backlog-processing-started = Processing backlog tasks
backlog-none-pending = No pending backlog tasks
backlog-pending-found = Found { $count } pending backlog tasks
backlog-team-processing = Processing { $count } backlog tasks for team { $team_id }
backlog-processing-summary = Backlog processing finished: reactivated={ $reactivated }, expired={ $expired }, retry_exhausted={ $retry_exhausted }, denied={ $denied }, queued={ $queued }, skipped={ $skipped }, failed={ $failed }

# Backlog worker detailed logs (20 keys)
backlog-processing-failed = Processing backlog task failed: { $error }
backlog-expired-marked = Backlog task { $task_id } expired, marked as expired
backlog-retry-exhausted-marked = Backlog task { $task_id } reached max retries, marked as failed
backlog-concurrency-available = Team { $team_id } concurrency slot available, processing backlog task { $task_id }
backlog-state-transition-skipped = Backlog task { $task_id } cannot transition to Processing (current { $current }): { $error }, skipped
backlog-reactivated = Backlog task { $task_id } reactivated successfully
backlog-reactivation-failed = Reactivating task failed: { $error }
backlog-concurrency-denied = Team { $team_id } concurrency limit not released: { $reason }, backlog task { $task_id } stays in backlog
backlog-unexpected-requeue = Backlog task { $task_id } was re-queued, unexpected behavior
backlog-concurrency-check-failed = Checking team concurrency limit failed: { $error }
backlog-task-status-not-activatable = Task { $task_id } status is { $status }, no reactivation needed
backlog-task-reactivated = Task { $task_id } reactivated successfully
backlog-cleanup-started = Starting expired backlog cleanup
backlog-cleanup-none = No expired backlog tasks
backlog-cleanup-failed = Expired backlog cleanup failed: { $error }
backlog-cleanup-completed = Expired backlog cleanup finished, cleaned { $count } tasks
backlog-cleanup-processing = Processing expired backlog task { $task_id }
backlog-task-marked-failed-by-expiry = Task { $task_id } marked as failed due to backlog expiry
backlog-process-error = Error while processing backlog tasks: { $error }
backlog-cleanup-error = Error while cleaning expired backlog tasks: { $error }

# Link extractor logs (2 keys)
link-score-skip = task_id: { $task_id }, URL scoring failed, skipped: { $error } ({ $url })
link-frontier-enqueued = task_id: { $task_id }, { $count } URLs entered Frontier ({ $domains } domains), dequeued by score

# Scrape executor logs (5 keys)
scrape-exec-encoding-started = Starting text encoding conversion, task ID: { $task_id }, URL: { $url }
scrape-exec-encoding-succeeded = Text encoding processing succeeded, detected encoding: { $encoding }, processing time: { $elapsed }ms, quality score: { $quality }
scrape-exec-encoding-failed = Text encoding processing failed: { $error }
scrape-exec-encoding-error = Text encoding processing exception: { $error }
scrape-task-encoding-fallback = Text encoding processing failed, using original content: { $error }
