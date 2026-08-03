# Error messages — en-US
# Covers all CrawlRsError::user_message() variants (17 keys)

error-database = Database operation failed. Please try again later.
error-network = External service unavailable. Please try again later.
error-config = Configuration error. Please contact support.
error-validation = Validation error: { $message }
error-not-found = Resource not found: { $resource }
error-auth = Authentication failed: { $reason }
error-permission = Permission denied.
error-timeout = Request timed out. Please try again later.
error-rate-limit = Rate limit exceeded. Please slow down your requests.
error-quota = Quota exceeded. Please upgrade your plan.
error-service-unavailable = Service unavailable. Please try again later.
error-cache = Cache service unavailable. Please try again later.
error-task = Task processing error. Please try again later.
error-json = Invalid JSON format. Please check your request.
error-io = Internal I/O error. Please try again later.
error-engine = Engine error occurred.
error-internal = Internal server error. Please try again later.

# Domain error messages (17 keys)

domain-error-crawl-config = Crawler configuration error: { $message }
domain-error-depth-exceeded = Crawl depth exceeded: maximum { $max }, requested { $requested }.
domain-error-path-filtered = URL path excluded by filter rules: { $path }
domain-error-task-not-found = Task not found: { $task_id }
domain-error-invalid-task-state = Invalid task state: current { $current }, expected { $expected }.
domain-error-task-expired = Task expired.
domain-error-team-not-found = Team not found: { $team_id }
domain-error-insufficient-credits = Insufficient credits: required { $required }, available { $available }.
domain-error-concurrency-limit = Team concurrency limit exceeded: current { $current }, limit { $limit }.
domain-error-invalid-url = Invalid URL: { $url }
domain-error-domain-blacklisted = URL is blacklisted: { $domain }
domain-error-robots-forbidden = URL is forbidden by robots.txt: { $url }
domain-error-webhook-delivery-failed = Webhook delivery failed.
domain-error-invalid-webhook-url = Invalid webhook URL: { $url }
domain-error-llm-extraction-failed = LLM extraction failed.
domain-error-invalid-css-selector = Invalid CSS selector: { $selector }
domain-error-validation = Validation error: { $field } - { $message }
