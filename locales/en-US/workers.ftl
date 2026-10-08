# Worker operational log messages — en-US
# Covers BacklogWorker::process_backlog() log statements (5 keys),
# backlog/link-extractor/scrape-executor details (27 keys) and F11 sitemap
# request source / conditional-request logs (15 keys)

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

# Crawl task: soft 404 & sitemap request source (7 keys)
crawl-soft404-detected = soft 404 detected for url { $url } (task { $task_id }): matching shell fingerprint, result not persisted
crawl-sitemap-derive-failed = sitemap_seed crawl { $crawl_id } cannot derive sitemap url from { $url }; sitemap request source skipped
crawl-sitemap-blacklist-unparsable = crawl { $crawl_id }: domain_blacklist payload unparsable ({ $error }); sitemap feed skipped — restriction cannot be verified
crawl-sitemap-feeding = sitemap request source for crawl { $crawl_id }: feeding { $sitemap_url }
crawl-sitemap-feed-resumed = sitemap feed for crawl { $crawl_id } resumed from loc { $loc } (pointer { $pointer })
crawl-sitemap-feed-done = sitemap feed done for crawl { $crawl_id }: enqueued={ $enqueued }, filtered={ $filtered }, duplicates={ $duplicates }, consumed={ $consumed } from { $sitemap_url }
crawl-sitemap-feed-failed = sitemap request source failed for crawl { $crawl_id } ({ $sitemap_url }): { $error }; crawl continues with page-link discovery only

# Scrape task: conditional request & 304 reuse (3 keys)
scrape-conditional-lookup-failed = Conditional request: prior result lookup failed for task { $task_id }, degrading to unconditional fetch: { $error }
scrape-304-reused = Task { $task_id } got 304 Not Modified, reusing prior result ({ $bytes } bytes)
scrape-304-no-prior = Task { $task_id } got 304 without prior result to reuse, marking failed

# Sitemap feed pointer (3 keys)
sitemap-pointer-mismatch = sitemap feed pointer for crawl { $crawl_id } targets { $pointer_url } but feeding { $sitemap_url }; ignoring — dedup/DB checks guard against re-enqueue
sitemap-pointer-corrupt = sitemap feed pointer for crawl { $crawl_id } is corrupt ({ $error }); ignoring
sitemap-resume-without-seek = sitemap feed for crawl { $crawl_id } resumes without seek (index streams arrive in nondeterministic order; dedup guards re-enqueue), pointer consumed={ $consumed }

# Soft 404 detector (2 keys)
soft404-already-initialized = soft404 detector already initialized; ignoring soft404_max_fingerprints={ $max_fingerprints } (restart required to apply)
soft404-hit = soft 404 hit: status { $status } response matches shell learned from HTTP { $learned_from } (content_length={ $content_length }, body_hash={ $body_hash }, fingerprint hits={ $hits }, total hits={ $total })
