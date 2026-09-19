# Operational log messages (non-worker) — en-US
# Covers text-processing pipeline, bootstrap CORS and engine runtime logs (29 keys)

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
