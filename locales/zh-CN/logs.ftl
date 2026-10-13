# 运维日志消息（非 worker）— zh-CN
# 覆盖文本处理流水线、启动期、引擎运行时、搜索、安全与可观测性日志（29 + 49 = 78 keys）

# 文本处理流水线（15 keys）
text-short-input = 处理短文本，长度: { $length } 字节
text-long-input = 处理长文本，长度: { $length } 字节
text-cached-detection = 使用缓存的编码检测结果: { $detection }
text-unicode-escapes-detected = 检测到Unicode转义序列，执行规范化转换
text-unicode-escapes-parsed = Unicode转义序列解析完成: { $result }
text-detection-started = 开始编码检测
text-encoding-detected = 检测到编码: { $encoding }，置信度: { $confidence }
text-detection-low-confidence = 编码检测置信度过低: { $confidence }，使用UTF-8尝试解析
text-html-structure-detected = 检测到HTML结构: { $is_html }，声明编码: { $encoding }
text-encoding-succeeded = 文本编码处理成功
text-encoding-failed = 文本编码处理失败: { $error }
text-web-content-started = 开始处理网页内容，大小: { $size } 字节
text-crawl-started = 开始处理抓取的内容: URL={ $url }，大小={ $size } 字节
text-crawl-completed = 内容处理完成: URL={ $url }，耗时={ $elapsed }，提取文本长度={ $length }
text-config-updated = 更新爬虫文本处理器配置: { $config }

# 爬虫文本集成（7 keys）
text-integration-enabled = 启用爬虫文本处理功能
text-integration-disabled = 禁用爬虫文本处理功能
text-integration-disabled-direct = 文本处理功能已禁用，直接返回原始内容
text-integration-succeeded = 文本处理成功: URL={ $url }，提取文本长度={ $length }，语言={ $language }
text-integration-failed = 文本处理失败: URL={ $url }，错误={ $error }
text-integration-disabled-batch = 文本处理功能已禁用，批量返回原始内容
text-integration-item-failed = 批量处理中的单个项目失败: URL={ $url }，错误={ $error }

# 启动期（2 keys）
boot-cors-wildcard = CORS 使用通配符 '*'，建议在生产环境中配置具体的来源
boot-cors-invalid-fallback = CORS 配置无效，允许所有来源作为回退

# 浏览器下载器日志（4 keys）
browser-check-started = 开始检查浏览器...
browser-fetcher-download-failed = fetcher 下载失败: { $error }
browser-cleaned-up = 已清理下载的浏览器
browser-system-found = 找到系统浏览器: { $path }

# 引擎路由（1 key）
engine-registered = 引擎已注册: { $name }

# FlareSolverr 引擎（5 个 key）
flare-url-not-configured = FlareSolverr URL 未配置（FLARESOLVERR_URL 环境变量为空或未设置）。使用占位符 '{ $fallback }'。请在 config/default.toml 中设置 CRAWLRS__ENGINES__FLARESOLVERR__URL，或通过环境变量设置。
flare-url-validation-failed = FlareSolverr URL 校验失败（'{ $url }'）: { $error }。回退到占位符 '{ $fallback }'。
flare-url-validation-fallback = FlareSolverr URL 校验失败: { $error }。回退到 '{ $fallback }'。
flare-mode-url-validation-fallback = FlareSolverr { $mode } URL 校验失败: { $error }。回退到 '{ $fallback }'。
flare-url-not-configured-fallback = FlareSolverr URL 未配置。使用占位符 '{ $fallback }'。

# 会话身份池（3 个 key）
session-identity-retired = session: 身份 #{ $id } 已退休（score={ $score }，uses={ $uses }，age={ $age }），替换为 #{ $fresh_id }
session-identity-force-retired = session: 身份 #{ $id } 已对 host { $host } 强制退休（score={ $score }，uses={ $uses }）
session-cookie-cap-dropped = session: 丢弃 set-cookie 对（{ $bytes } 字节），超出身份 #{ $id } 的缓存上限 { $cap }

# 浏览器等待策略（2 个 key）
wait-network-idle-subscribe-failed = network idle: CDP 事件订阅失败（{ $error }）；回退为固定休眠
wait-dom-unstable-degraded = DOM 在 { $timeout }ms 后仍未稳定，降级处理（尽力等待）

# Playwright 引擎（9 个 key）
playwright-stealth-inject-failed = 隐身注入失败（尽力而为，继续）: { $error }
playwright-xhr-setup-failed = XHR 捕获: Network 域设置失败（{ $error }）；本页禁用捕获
playwright-xhr-dropped = XHR 捕获丢弃: 超限={ $oversize }，解码失败={ $decode_failed }，取体失败={ $fetch_failed }，溢出={ $overflow }
playwright-xhr-internal-blocked = XHR 捕获: 内网 URL { $url } 已拦截（SSRF）
playwright-xhr-unfinished-at-collect = XHR 捕获: 收集时 { $count } 个 XHR 未完成，已丢弃
playwright-xhr-body-oversize = XHR 捕获: { $url } 的响应体超出 max_bytes，已丢弃
playwright-xhr-body-decode-failed = XHR 捕获: { $url } 的响应体 base64 解码失败
playwright-xhr-fetch-body-failed = XHR 捕获: { $url } 的 getResponseBody 失败: { $error }
playwright-handler-event-error = 浏览器 handler 事件错误（继续运行）: { $error }

# 浏览器池（1 个 key）
playwright-pool-drop-no-runtime = BrowserPool drop 时无 tokio runtime；浏览器实例与 profile 目录未被清理

# wreq TLS 仿真（1 个 key）
wreq-non-chromium-emulation = 非 Chromium 的 TlsEmulation { $emulation } 进入 emulation_provider；回退为 Chromium 仿真（UA 池过滤本应阻止此情况）

# SSRF 防护（1 个 key）
ssrf-protection-disabled = SSRF 防护已通过 { $env } 禁用 — 内网/私有网络目标可达；仅限开发/测试使用

# robots.txt（1 个 key）
robots-fetch-fail-closed = 从 { $url } 获取 robots.txt 失败（fail-closed）: { $error }

# MLLM CDP 桥接错误载荷（4 个 key）
mllm-ax-tree-failed = GetFullAXTree 失败: { $error }
mllm-dom-resolve-node-failed = DOM.resolveNode 失败: { $error }
mllm-call-function-params-invalid = callFunctionOn 参数无效: { $error }
mllm-call-function-click-failed = callFunctionOn 点击失败: { $error }

# 分布式 ID 服务（3 个 key）
id-refill-failed = NebulaId 补水失败: { $error }
id-global-generator-initialized = NebulaId 全局同步生成器已初始化（池已播种，workspace={ $workspace }，group={ $group }）
id-service-initialized = IdService 已初始化（algorithm={ $algorithm }，workspace={ $workspace }，group={ $group }）

# 启动期与 map 端点日志（3 个 key）
boot-playwright-capture-enabled = Playwright XHR/Fetch 捕获已启用（patterns={ $patterns }，max_bytes={ $max_bytes }，max_responses={ $max_responses }）
boot-register-id-module-failed = 注册 IdModule 失败
map-target-unreachable = map 目标不可达 { $url }: { $message }

# 搜索引擎与智能路由（8 个 key）
search-exa-registered = Exa 引擎已注册 (engine="exa")
search-exa-skipped = Exa 引擎已跳过: api_key 未配置（设置 CRAWLRS__SEARCH__EXA__API_KEY 以启用 engine="exa"）
search-tavily-registered = Tavily 引擎已注册 (engine="tavily")
search-tavily-skipped = Tavily 引擎已跳过: api_key 未配置（设置 CRAWLRS__SEARCH__TAVILY__API_KEY 以启用 engine="tavily"）
search-smart-engine-timeout = 引擎在智能并发扇出中超时（{ $timeout }），降级处理
search-smart-start = 智能搜索开始: query={ $query }，limit={ $limit }，lang={ $lang }，country={ $country }，offset={ $offset }
search-smart-cross-dedup = 跨引擎标题去重移除 { $count } 条近似重复项
search-smart-not-injected = 智能引擎未注入；回退到单引擎搜索路径

# 失败快照旁路（5 个 key）
snapshot-join-failed = 失败快照: 阻塞任务 join 失败: { $error }
snapshot-dir-create-failed = 失败快照: 无法创建目录 { $dir }: { $error }
snapshot-write-failed = 失败快照: 写入 { $path } 失败: { $error }
snapshot-read-dir-failed = 失败快照: 读取目录 { $dir } 失败: { $error }
snapshot-evict-failed = 失败快照: 淘汰 { $path } 失败: { $error }

# 自适应并发（AIMD）（3 个 key）
aimd-limit-increased = AIMD 上限提升: { $from } -> { $to }
aimd-limit-halved = AIMD 上限减半: { $from } -> { $to }
aimd-target-changed = AIMD 信号量目标: { $from } -> { $to }

# 搜索引擎开关 / 默认引擎（配置消费，文档审计缺陷 #3）
search-engine-disabled-by-config = 内置搜索引擎 '{ $engine }' 已被配置关闭，跳过注册
search-all-builtin-engines-disabled = 全部内置搜索引擎均被配置关闭；smart 聚合与单引擎路径将不可用
search-default-engine-not-registered = search.default_engine '{ $engine }' 未注册（被关闭或缺少凭据）；回退 bing
search-default-engine-unknown = search.default_engine '{ $engine }' 不是已知引擎名（bing/baidu/sogou/exa/tavily）；回退 bing

# auth-off 降级模式（2 个 key）
auth-allow-unauthenticated-enabled = allow_unauthenticated_protected() 显式启用：protected routes 将放行未鉴权请求（匿名受限身份）。仅限受信任内网部署；公开网络必须启用 auth feature。
auth-disabled-unauthenticated-denied = auth feature 已禁用：protected route 收到未鉴权请求，默认拒绝（401）。若确需匿名放行，请显式调用 allow_unauthenticated_protected()。
