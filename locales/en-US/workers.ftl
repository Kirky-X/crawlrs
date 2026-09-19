# Worker operational log messages — en-US
# Covers BacklogWorker::process_backlog() log statements (5 keys)

backlog-processing-started = Processing backlog tasks
backlog-none-pending = No pending backlog tasks
backlog-pending-found = Found { $count } pending backlog tasks
backlog-team-processing = Processing { $count } backlog tasks for team { $team_id }
backlog-processing-summary = Backlog processing finished: reactivated={ $reactivated }, expired={ $expired }, retry_exhausted={ $retry_exhausted }, denied={ $denied }, queued={ $queued }, skipped={ $skipped }, failed={ $failed }
