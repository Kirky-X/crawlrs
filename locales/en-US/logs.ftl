# Operational log messages (non-worker) — en-US
# Covers text-processing pipeline, bootstrap, engine runtime, search, security
# and observability logs (29 + 49 = 78 keys)

# Text processing pipeline (15 keys)
text-short-input = Processing short text, length: { $length } bytes
text-long-input = Processing long text, length: { $length } bytes
text-cached-detection = Using cached encoding detection result: { $detection }
text-unicode-escapes-detected = Unicode escape sequences detected, performing normalization conversion
text-unicode-escapes-parsed = Unicode escape sequence parsing finished: { $result }
text-detection-started = Starting encoding detection
text-encoding-detected = Detected encoding: { $encoding }, confidence: { $confidence }
text-detection-low-confidence = Encoding detection confidence too low: { $confidence }, falling back to UTF-8 parsing
text-html-structure-detected = Detected HTML structure: { $is_html }, declared encoding: { $encoding }
text-encoding-succeeded = Text encoding processing succeeded
text-encoding-failed = Text encoding processing failed: { $error }
text-web-content-started = Starting web content processing, size: { $size } bytes
text-crawl-started = Starting crawled content processing: URL={ $url }, size={ $size } bytes
text-crawl-completed = Content processing finished: URL={ $url }, elapsed={ $elapsed }, extracted text length={ $length }
text-config-updated = Updated crawl text processor config: { $config }

# Crawler text integration (7 keys)
text-integration-enabled = Crawler text processing enabled
text-integration-disabled = Crawler text processing disabled
text-integration-disabled-direct = Text processing disabled, returning original content directly
text-integration-succeeded = Text processing succeeded: URL={ $url }, extracted text length={ $length }, language={ $language }
text-integration-failed = Text processing failed: URL={ $url }, error={ $error }
text-integration-disabled-batch = Text processing disabled, returning original content in batch
text-integration-item-failed = Single item failed in batch processing: URL={ $url }, error={ $error }

# Bootstrap (2 keys)
boot-cors-wildcard = CORS uses wildcard '*', configure specific origins for production
boot-cors-invalid-fallback = Invalid CORS config, allowing all origins as fallback

# Browser downloader logs (4 keys)
browser-check-started = Starting browser check...
browser-fetcher-download-failed = fetcher download failed: { $error }
browser-cleaned-up = Cleaned up downloaded browser
browser-system-found = Found system browser: { $path }

# Engine router (1 key)
engine-registered = Engine registered: { $name }

# FlareSolverr engine (5 keys)
flare-url-not-configured = FlareSolverr URL not configured (FLARESOLVERR_URL env var empty or unset). Using placeholder '{ $fallback }'. Set CRAWLRS__ENGINES__FLARESOLVERR__URL in config/default.toml or via environment variable.
flare-url-validation-failed = FlareSolverr URL validation failed ('{ $url }'): { $error }. Falling back to placeholder '{ $fallback }'.
flare-url-validation-fallback = FlareSolverr URL validation failed: { $error }. Falling back to '{ $fallback }'.
flare-mode-url-validation-fallback = FlareSolverr { $mode } URL validation failed: { $error }. Falling back to '{ $fallback }'.
flare-url-not-configured-fallback = FlareSolverr URL not configured. Using placeholder '{ $fallback }'.

# Session identity pool (3 keys)
session-identity-retired = session: identity #{ $id } retired (score={ $score }, uses={ $uses }, age={ $age }), replacing with #{ $fresh_id }
session-identity-force-retired = session: identity #{ $id } force-retired for host { $host } (score={ $score }, uses={ $uses })
session-cookie-cap-dropped = session: dropping set-cookie pair ({ $bytes } bytes) exceeding cache cap { $cap } for identity #{ $id }

# Browser wait strategies (2 keys)
wait-network-idle-subscribe-failed = network idle: CDP event subscription failed ({ $error }); falling back to fixed sleep
wait-dom-unstable-degraded = dom not stable after { $timeout }ms, degrading (best-effort wait)

# Playwright engine (9 keys)
playwright-stealth-inject-failed = Stealth injection failed (best-effort, continue): { $error }
playwright-xhr-setup-failed = XHR capture: Network setup failed ({ $error }); capture disabled for this page
playwright-xhr-dropped = XHR capture dropped: oversize={ $oversize }, decode_failed={ $decode_failed }, fetch_failed={ $fetch_failed }, overflow={ $overflow }
playwright-xhr-internal-blocked = XHR capture: internal URL { $url } blocked (SSRF)
playwright-xhr-unfinished-at-collect = XHR capture: { $count } XHR(s) unfinished at collect, dropped
playwright-xhr-body-oversize = XHR capture: body of { $url } exceeds max_bytes, dropped
playwright-xhr-body-decode-failed = XHR capture: body of { $url } base64 decode failed
playwright-xhr-fetch-body-failed = XHR capture: getResponseBody failed for { $url }: { $error }
playwright-handler-event-error = browser handler event error (continuing): { $error }

# Browser pool (1 key)
playwright-pool-drop-no-runtime = No tokio runtime on BrowserPool drop; browser instances and profile dirs are not cleaned up

# wreq TLS emulation (1 key)
wreq-non-chromium-emulation = non-Chromium TlsEmulation { $emulation } reached emulation_provider; falling back to Chromium emulation (UA pool filter should prevent this)

# SSRF protection (1 key)
ssrf-protection-disabled = SSRF protection DISABLED via { $env } — internal/private network targets are reachable; for development/testing only

# robots.txt (1 key)
robots-fetch-fail-closed = Failed to fetch robots.txt from { $url } (fail-closed): { $error }

# MLLM CDP bridge error payloads (4 keys)
mllm-ax-tree-failed = GetFullAXTree failed: { $error }
mllm-dom-resolve-node-failed = DOM.resolveNode failed: { $error }
mllm-call-function-params-invalid = invalid callFunctionOn params: { $error }
mllm-call-function-click-failed = callFunctionOn click failed: { $error }

# Distributed ID service (3 keys)
id-refill-failed = NebulaId refill failed: { $error }
id-global-generator-initialized = NebulaId global sync generator initialized (pool seeded, workspace={ $workspace }, group={ $group })
id-service-initialized = IdService initialized (algorithm={ $algorithm }, workspace={ $workspace }, group={ $group })

# Bootstrap & map endpoint logs (3 keys)
boot-playwright-capture-enabled = Playwright XHR/Fetch capture enabled (patterns={ $patterns }, max_bytes={ $max_bytes }, max_responses={ $max_responses })
boot-register-id-module-failed = register IdModule failed
map-target-unreachable = map target unreachable for { $url }: { $message }

# Search engines & smart routing (8 keys)
search-exa-registered = Exa engine registered (engine="exa")
search-exa-skipped = Exa engine skipped: api_key not configured (set CRAWLRS__SEARCH__EXA__API_KEY to enable engine="exa")
search-tavily-registered = Tavily engine registered (engine="tavily")
search-tavily-skipped = Tavily engine skipped: api_key not configured (set CRAWLRS__SEARCH__TAVILY__API_KEY to enable engine="tavily")
search-smart-engine-timeout = engine timed out after { $timeout } in smart fan-out, degrading
search-smart-start = Smart search start: query={ $query }, limit={ $limit }, lang={ $lang }, country={ $country }, offset={ $offset }
search-smart-cross-dedup = Cross-engine title dedup removed { $count } near-duplicates
search-smart-not-injected = smart engine not injected; falling back to single-engine search path

# Failure snapshot bypass (5 keys)
snapshot-join-failed = failure snapshot: blocking task join failed: { $error }
snapshot-dir-create-failed = failure snapshot: cannot create dir { $dir }: { $error }
snapshot-write-failed = failure snapshot: write { $path } failed: { $error }
snapshot-read-dir-failed = failure snapshot: read_dir { $dir } failed: { $error }
snapshot-evict-failed = failure snapshot: evict { $path } failed: { $error }

# Adaptive concurrency (AIMD) (3 keys)
aimd-limit-increased = AIMD limit increased: { $from } -> { $to }
aimd-limit-halved = AIMD limit halved: { $from } -> { $to }
aimd-target-changed = AIMD semaphore target: { $from } -> { $to }

# Search engine switches / default engine (config consumption, doc-audit defect #3)
search-engine-disabled-by-config = Built-in search engine '{ $engine }' disabled by config, skipping registration
search-all-builtin-engines-disabled = All builtin search engines are disabled by config; smart aggregation and single-engine search will be unavailable
search-default-engine-not-registered = search.default_engine '{ $engine }' is not registered (disabled or missing credentials); falling back to bing
search-default-engine-unknown = search.default_engine '{ $engine }' is not a known engine name (bing/baidu/sogou/exa/tavily); falling back to bing
