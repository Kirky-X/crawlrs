// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 抓取管线 — 从 `ScrapeWorker` 拆分的 partial impl block
//!
//! 包含请求准备与校验、条件请求与缓存读写、成功/失败收尾。

use super::*;

impl ScrapeWorker {
    /// crawl 终态闸（scrape/crawl 两条任务路径共用）。
    ///
    /// 命中终态时用 `cancel_tasks_by_crawl_id` 一次性取消该 crawl 的
    /// queued/active 残留任务（单条 UPDATE），返回 true 表示当前任务应终止。
    ///
    /// 缓存：crawl_id → (is_terminal, checked_at)，10s TTL；过期项读取时移除，
    /// 每 1024 次插入对整个 map retain 一次过期项（防无界增长）。
    pub(super) async fn crawl_terminal_short_circuit(&self, task: &Task) -> Result<bool> {
        const CRAWL_TERMINAL_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(10);

        let Some(crawl_id) = task.crawl_id else {
            return Ok(false);
        };

        let cached_terminal = self.crawl_terminal_cache.get(&crawl_id).and_then(|entry| {
            if entry.value().1.elapsed() < CRAWL_TERMINAL_CACHE_TTL {
                Some(entry.value().0)
            } else {
                None
            }
        });
        if cached_terminal.is_none() {
            // 过期项摘除（DashMap 无全局 GC，靠读取与容量阈值兜底）
            self.crawl_terminal_cache.remove(&crawl_id);
        }
        let is_terminal = match cached_terminal {
            Some(value) => value,
            // 只用状态：投影读取，避免每任务拉回 crawl.config。
            None => match self.crawl_repository.find_progress_by_id(crawl_id).await {
                Ok(Some(progress)) => {
                    let value = crate::application::use_cases::crawl_state_machine::is_terminal(
                        &progress.status,
                    );
                    self.crawl_terminal_cache
                        .insert(crawl_id, (value, std::time::Instant::now()));
                    self.note_crawl_terminal_cache_insert(CRAWL_TERMINAL_CACHE_TTL);
                    value
                }
                // crawl 行不存在（无外键的孤儿任务）同样缓存，避免每任务打库
                Ok(None) => {
                    self.crawl_terminal_cache
                        .insert(crawl_id, (false, std::time::Instant::now()));
                    self.note_crawl_terminal_cache_insert(CRAWL_TERMINAL_CACHE_TTL);
                    false
                }
                Err(e) => {
                    log::warn!(
                        "crawl_terminal_gate lookup failed for crawl {}: {}",
                        crawl_id,
                        e
                    );
                    false
                }
            },
        };

        if !is_terminal {
            return Ok(false);
        }

        info!(
            "crawl_terminal_gate: cancel task {} (crawl {} is terminal)",
            task.id, crawl_id
        );
        match self.repository.cancel_tasks_by_crawl_id(crawl_id).await {
            Ok(0) => {
                // 当前任务可能已被他方终结；兜底单条取消
                self.repository.mark_cancelled(task.id).await?;
            }
            Ok(cancelled) => {
                info!(
                    "crawl_terminal_gate: cancelled {} residual task(s) for crawl {}",
                    cancelled, crawl_id
                );
            }
            Err(e) => {
                log::warn!(
                    "crawl_terminal_gate batch cancel failed for crawl {}: {}",
                    crawl_id,
                    e
                );
                self.repository.mark_cancelled(task.id).await?;
            }
        }
        Ok(true)
    }

    /// 每 1024 次插入做一次过期项清理（O(1) 递增，不逐任务扫描分片）。
    fn note_crawl_terminal_cache_insert(&self, ttl: std::time::Duration) {
        let count = self
            .crawl_terminal_cache_inserts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        if count.is_multiple_of(1024) {
            self.crawl_terminal_cache
                .retain(|_, value| value.1.elapsed() < ttl);
        }
    }

    pub(super) async fn process_scrape_task(&self, mut task: Task) -> Result<()> {
        debug!("task_id: {}", task.id);

        // 终态闸（抽为共享方法，crawl 任务路径同样调用——
        // 此前只挂在 process_scrape_task，而 crawl 子任务走
        // process_crawl_task，闸在生产路径不可达）。
        if self.crawl_terminal_short_circuit(&task).await? {
            return Ok(());
        }

        let (scrape_request_dto, mut scrape_request) =
            match self.prepare_scrape_request(&task).await? {
                Some(pair) => pair,
                None => return Ok(()),
            };

        if !self
            .validate_scrape_request(&task, &scrape_request, scrape_request_dto.as_ref())
            .await?
        {
            return Ok(());
        }

        // 请求合并——同合并键（租户 + 规范化 URL + 完整请求身份指纹）并发只允许
        // 首个执行实际抓取
        //
        // 调用 CoalesceCoordinator（独立组件），返回 `Some(guard)` 表示获得执行权。
        // guard 默认按失败广播；只有结果成功落库后才调用 `mark_success`，
        // 等待方据此决定是否查询结果（失败路径不做无效查询）。
        // 返回 `None` 表示已被其他 worker 处理（等待方从 result_repo 读到结果，
        // 或任务被延后重排），调用方应直接返回 Ok。
        //
        // 键取自本任务实际会下发的请求：task.team_id（租户隔离，M-2）、
        // scrape_request.url（与实际下发 URL 一致，URL 归一化在 CoalesceKey 内完成）、
        // scrape_request.options 全字段（headers/请求级 proxy/method/body/needs_js/...
        // 任一不同都不得互相收养结果；ProxyProvider 运行时选路不入键）。
        let coalesce_key =
            CoalesceKey::new(task.team_id, &scrape_request.url, &scrape_request.options);
        let mut coalesce_guard = match self
            .coalesce_coordinator
            .try_coalesce(&coalesce_key, &task, self.worker_id)
            .await?
        {
            Some(g) => g,
            None => return Ok(()),
        };

        // 抓取成功/失败回填 AIMDController
        //
        // 由 `TeamSemaphore` 封装——Fixed 模式 noop；Adaptive 模式调用
        // `AIMDController::record_*` 并经 `AdaptiveSemaphore::set_target` 推入新 target。
        // guard 在 match 块作用域结束时 Drop，确保先广播给等待方再释放。

        let prior_result = self
            .prepare_conditional_request(&task, &mut scrape_request, scrape_request_dto.as_ref())
            .await;

        let response = self.scrape_with_cache(task.team_id, &scrape_request).await;

        match response {
            Ok(response) => {
                let finalized = self
                    .finalize_scrape_success(
                        &mut task,
                        &scrape_request,
                        scrape_request_dto.as_ref(),
                        prior_result,
                        response,
                    )
                    .await;
                // 仅当 leader 结果已落库（含守卫丢失但结果已保存）才广播成功；
                // 304 无历史结果 / 成功处理器失败等回退路径广播失败信号。
                if matches!(finalized, Ok(true)) {
                    coalesce_guard.mark_success();
                }
                finalized.map(|_| ())
            }
            // 抓取失败：结果未落库，guard 保持默认失败标记后 Drop
            Err(e) => self.finalize_scrape_failure(&mut task, e).await,
        }
    }

    /// 解析任务 payload 并构造 [`ScrapeRequest`]。
    ///
    /// PERF- 解析一次 payload，dto 与 ScrapeRequest 复用同一份解析结果。
    ///
    /// 旧实现：
    /// 1) build_scrape_request(&task) 内部 from_value(payload.clone) 得 dto
    /// 2) handle_scrape_success(&task, &response) 内部再 from_value(payload.clone) 得 dto
    ///
    /// → 每次抓取 clone + 解析 payload 2 次。
    ///
    /// 新实现：
    /// 1) parse_scrape_request_dto(&task) 得 dto（仅 1 次 clone + 解析）
    /// 2) build_scrape_request_from_dto(&dto) 复用 dto 构造 ScrapeRequest（零额外解析）
    /// 3) handle_scrape_success(&task, dto.as_ref, &response) 复用 dto 引用（零额外解析）
    ///
    /// → 每次抓取 clone + 解析 payload 1 次。
    ///
    /// 返回 `Ok(None)` 表示 payload 反序列化失败，任务已标记 Failed，调用方直接返回。
    pub(super) async fn prepare_scrape_request(
        &self,
        task: &Task,
    ) -> Result<Option<(Option<ScrapeRequestDto>, ScrapeRequest)>> {
        match Self::parse_scrape_request_dto(task) {
            Ok(dto) => {
                let req = match Self::build_scrape_request_from_dto(&dto) {
                    Ok(r) => r,
                    Err(e) => {
                        error!(
                            "Failed to build scrape request from dto, using default: {}",
                            e
                        );
                        ScrapeRequest::new(task.url.clone()).timeout(Duration::from_secs(
                            self.settings.timeouts.engines.default_timeout_seconds,
                        ))
                    }
                };
                Ok(Some((Some(dto), req)))
            }
            Err(e) => {
                // payload 反序列化失败即失败：不再静默回退
                // 默认 ScrapeRequest 带残缺配置执行，改为标记任务 Failed 并终止。
                // 仅输出 task_id 与错误摘要，绝不打印原始 payload（可能含敏感/超大内容）。
                error!(
                    "payload_invalid: task {} marked failed after payload deserialization error: {}",
                    task.id, e
                );
                // 失败原因入 payload["error"]：API 状态端点据此回显，
                // 否则只回退通用"任务失败。"（robots/SSRF 早退同源）。
                self.repository
                    .mark_failed_with_reason(task, &format!("Invalid task payload: {e}"))
                    .await?;
                Ok(None)
            }
        }
    }

    /// SSRF 防护与 robots.txt 门控。
    ///
    /// 返回 `Ok(false)` 表示任务已标记 Failed（SSRF 命中或 robots Disallow），
    /// 调用方直接返回。
    pub(super) async fn validate_scrape_request(
        &self,
        task: &Task,
        scrape_request: &ScrapeRequest,
        scrape_request_dto: Option<&ScrapeRequestDto>,
    ) -> Result<bool> {
        // SSRF 防护 (CWE-918)：静态校验 options.proxy 不指向内部网络（防御纵深）。
        // handler 层已通过 validate_url 完成完整 DNS 解析校验，
        // 此处仅用静态检查拦截直接入队的恶意任务（如 private IP / localhost），不依赖网络。
        if let Some(ref proxy_url) = scrape_request.options.proxy {
            if is_internal_url(proxy_url) {
                warn!(
                    "SSRF via proxy blocked in worker proxy={} task_id={} team_id={}",
                    redact_url_for_log(proxy_url),
                    task.id,
                    task.team_id
                );
                self.repository
                    .mark_failed_with_reason(
                        task,
                        "Blocked: proxy target is an internal address (SSRF protection)",
                    )
                    .await?;
                return Ok(false);
            }
        }

        // robots.txt 遵从（可选，默认关闭）：请求级 options.respect_robots 覆盖
        // 全局 robots.scrape_respect_robots。robots 获取失败 fail-open（与 crawl
        // 路径一致）；命中 Disallow 时任务置 Failed（原因记入日志，含 task_id）。
        // 单页抓取不执行 Crawl-delay（非爬虫逐链接语义）。
        let respect_override = scrape_request_dto
            .and_then(|dto| dto.options.as_ref())
            .and_then(|o| o.respect_robots);
        let robots_allowed = crate::workers::scrape_executor::check_scrape_robots_allowed(
            task,
            respect_override,
            self.settings.robots.scrape_respect_robots,
            self.settings.robots.user_agent.as_str(),
            self.robots_checker.as_ref(),
        )
        .await?;
        if !robots_allowed {
            warn!(
                "robots_disallowed: task {} marked failed, url={}, ua={}",
                task.id,
                redact_url_for_log(&task.url),
                self.settings.robots.user_agent
            );
            self.repository
                .mark_failed_with_reason(
                    task,
                    &format!(
                        "Blocked by robots.txt: URL disallowed for user-agent '{}'",
                        self.settings.robots.user_agent
                    ),
                )
                .await?;
            return Ok(false);
        }

        Ok(true)
    }

    /// 增量抓取（scoped 切片）：`conditional_request=true` 且同任务已有
    /// 带验证器的历史结果时，把 If-None-Match / If-Modified-Since 合并进
    /// 请求头发起条件请求；`304` 时复用历史结果内容（见 process_scrape_task
    /// 的短路分支）。历史查询失败降级为无条件抓取（warn 显性化，不阻塞抓取）。
    pub(super) async fn prepare_conditional_request(
        &self,
        task: &Task,
        scrape_request: &mut ScrapeRequest,
        scrape_request_dto: Option<&ScrapeRequestDto>,
    ) -> Option<crate::domain::models::ScrapeResult> {
        let mut prior_result: Option<crate::domain::models::ScrapeResult> = None;
        let conditional_enabled = scrape_request_dto
            .and_then(|d| d.options.as_ref())
            .and_then(|o| o.conditional_request)
            .unwrap_or(false);
        if conditional_enabled {
            match self.result_repository.find_by_task_id(task.id).await {
                Ok(Some(prior)) => {
                    if let Some((etag, last_modified)) = extract_cache_validators(&prior.headers) {
                        if !etag.is_empty() {
                            scrape_request
                                .options
                                .headers
                                .insert("If-None-Match".to_string(), etag);
                        }
                        if let Some(lm) = last_modified.filter(|m| !m.is_empty()) {
                            scrape_request
                                .options
                                .headers
                                .insert("If-Modified-Since".to_string(), lm);
                        }
                        prior_result = Some(prior);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        "{}",
                        crate::i18n::tr_log_args(
                            "scrape-conditional-lookup-failed",
                            &[("task_id", task.id.to_string(),), ("error", e.to_string()),],
                        )
                    );
                }
            }
        }
        prior_result
    }

    /// 高级缓存模式门控 + 引擎执行。
    ///
    /// 构造 `CacheContext`，按 `cache_mode` 决定读写行为：
    /// - 读缓存：`is_cacheable && should_read` → 查缓存，命中直返跳过 `engine_client.scrape`
    /// - 写缓存：`is_cacheable && should_write` 且抓取成功 → 序列化写回
    ///
    /// `cache_mode=None`（默认）等价于 `Enabled`（`unwrap_or_default()`）。
    /// 不可缓存的请求（data:/blob:/POST）跳过整个缓存流程；cache key 也只在
    /// 缓存流程实际启用（可缓存且可读或可写）时生成。
    ///
    /// `team_id` 进入 cache key：跨租户不共享缓存条目（CWE-200）。
    pub(super) async fn scrape_with_cache(
        &self,
        team_id: Id,
        scrape_request: &ScrapeRequest,
    ) -> Result<ScrapeResponse, EngineError> {
        let cache_ctx = CacheContext {
            team_id,
            url: scrape_request.url.clone(),
            method: scrape_request.options.method,
            mode: scrape_request.options.cache_mode.unwrap_or_default(),
        };

        // cache key 仅在缓存流程实际启用时生成：`is_cacheable` 且至少一个
        // 读写方向被允许。`Disabled` 与不可缓存请求（data:/blob:/POST 等）
        // 不做指纹计算。读/写共用同一个 `Option<String>`，避免门控前无条件
        // 生成 key（指纹含 JSON 规范化，属非平凡开销）。
        let cache_key = (cache_ctx.is_cacheable()
            && (cache_ctx.should_read() || cache_ctx.should_write()))
        .then(|| cache_utils::generate_scrape_cache_key(&cache_ctx, &scrape_request.options));

        // 读缓存门控
        if let Some(cached) = self
            .read_cached_scrape_response(&cache_ctx, cache_key.as_deref())
            .await
        {
            return Ok(cached);
        }

        let resp = self.engine_client.scrape(scrape_request).await;
        self.write_scrape_cache_on_success(&cache_ctx, cache_key.as_deref(), &resp)
            .await;
        resp
    }

    /// 缓存读取（自 [`ScrapeWorker::scrape_with_cache`] 提取）：
    /// 命中直返，读失败记录后降级为 miss（不阻塞抓取）。
    pub(super) async fn read_cached_scrape_response(
        &self,
        cache_ctx: &CacheContext,
        cache_key: Option<&str>,
    ) -> Option<ScrapeResponse> {
        match cache_key {
            Some(cache_key) if cache_ctx.should_read() => {
                match try_read_scrape_cache(cache_ctx, cache_key, self.cache_service.as_ref()).await
                {
                    Ok(Some(cached)) => {
                        // debug 禁用时跳过 redact_url_for_log 调用（~1μs）
                        // log crate 的 debug! 宏本身已 lazy format_args，但函数参数在宏调用前已求值，
                        // 需 log_enabled! 守卫才能避免 redact_url_for_log 的 Url::parse + String 分配。
                        if log::log_enabled!(log::Level::Debug) {
                            debug!(
                                "Cache hit, returning cached response url={} mode={:?}",
                                redact_url_for_log(&cache_ctx.url),
                                cache_ctx.mode
                            );
                        }
                        Some(cached)
                    }
                    Ok(None) => None,
                    Err(e) => {
                        // 缓存读失败不吞，记录后降级为 miss（不阻塞抓取）
                        // 日志使用脱敏 URL，防止 query 参数泄露
                        warn!(
                            "Cache read failed, falling back to scrape url={} error={}",
                            redact_url_for_log(&cache_ctx.url),
                            e
                        );
                        None
                    }
                }
            }
            _ => None,
        }
    }

    /// 抓取成功后写缓存（自 [`ScrapeWorker::scrape_with_cache`] 提取）：
    /// 仅抓取成功且 `should_write` 时写回；写失败记录但不影响抓取结果。
    pub(super) async fn write_scrape_cache_on_success(
        &self,
        cache_ctx: &CacheContext,
        cache_key: Option<&str>,
        resp: &Result<ScrapeResponse, EngineError>,
    ) {
        // 写缓存门控：仅抓取成功且 should_write 时写回
        if let Ok(r) = resp {
            if cache_ctx.should_write() {
                if let Some(cache_key) = cache_key {
                    if let Err(e) = try_write_scrape_cache(
                        cache_ctx,
                        cache_key,
                        r,
                        self.cache_service.as_ref(),
                        self.settings.cache.types.search.ttl_seconds,
                    )
                    .await
                    {
                        // 缓存写失败不吞，记录但不影响抓取结果
                        // 日志使用脱敏 URL
                        warn!(
                            "Cache write failed url={} error={}",
                            redact_url_for_log(&cache_ctx.url),
                            e
                        );
                    }
                }
            }
        }
    }

    /// 抓取成功路径：AIMD 记账 → 304 短路 → 持久化（handle_scrape_success）→
    /// 守卫胜利者扣除高级功能费用。
    ///
    /// 抓取成功/失败回填 AIMDController：由 `TeamSemaphore` 封装——Fixed 模式 noop；
    /// Adaptive 模式调用 `AIMDController::record_*` 并经 `AdaptiveSemaphore::set_target`
    /// 推入新 target。
    ///
    /// 返回值中的 `bool` 表示 leader 结果是否已落库（供 coalesce 广播成功信号）：
    /// - `Ok(true)`：结果已保存且任务已终结（含完成守卫丢失但结果已 idempotent
    ///   落库的场景）
    /// - `Ok(false)`：未保存结果（304 无历史结果、成功处理器失败等回退路径）
    /// - `Err`：仓储/处理器错误，调用方按失败处理
    pub(super) async fn finalize_scrape_success(
        &self,
        task: &mut Task,
        scrape_request: &ScrapeRequest,
        scrape_request_dto: Option<&ScrapeRequestDto>,
        prior_result: Option<crate::domain::models::ScrapeResult>,
        response: ScrapeResponse,
    ) -> Result<bool> {
        self.team_semaphore.record_success(task.team_id);

        // 304 短路：目标站确认内容未变，复用历史结果内容回填 200 响应，
        // 省去重复传输与重新提取。无历史结果可回填时为协议异常（304
        // 必然对应我们发出的条件请求），显性置 Failed 不返回空正文。
        let Some(response) = self
            .resolve_not_modified_response(task, prior_result, response)
            .await?
        else {
            return Ok(false);
        };

        debug!("status_code: {}", response.status_code);
        info!("Scrape successful, status: {}", response.status_code);

        // handle_scrape_success 改为 owned ScrapeResponse，
        // 调用前提前提取 has_screenshot 标志（response 将被 move 消费）
        let has_screenshot = response.screenshot.is_some();
        let has_proxy = scrape_request.options.proxy.is_some();

        self.persist_success_and_settle(
            task,
            scrape_request_dto,
            response,
            has_screenshot,
            has_proxy,
        )
        .await
    }

    /// 304 短路：命中历史结果时用其内容回填 200 响应；无历史结果按协议异常
    /// 置 Failed 并返回 `None`（调用方直接返回 `Ok(false)`）。
    async fn resolve_not_modified_response(
        &self,
        task: &mut Task,
        prior_result: Option<crate::domain::models::ScrapeResult>,
        response: ScrapeResponse,
    ) -> Result<Option<ScrapeResponse>> {
        if response.status_code != 304 {
            return Ok(Some(response));
        }

        match prior_result {
            Some(ref prior) => {
                info!(
                    "{}",
                    crate::i18n::tr_log_args(
                        "scrape-304-reused",
                        &[
                            ("task_id", task.id.to_string(),),
                            ("bytes", (prior.content.len() as i64).to_string(),),
                        ],
                    )
                );
                Ok(Some(not_modified_response(prior, &response)))
            }
            None => {
                error!(
                    "{}",
                    crate::i18n::tr_log_args(
                        "scrape-304-no-prior",
                        &[("task_id", task.id.to_string(),)],
                    )
                );
                self.handle_failure(
                    task,
                    "304 response without prior result for conditional request",
                )
                .await?;
                Ok(None)
            }
        }
    }

    /// 结果持久化与费用结算：success handler 失败转失败回填；守卫胜利者
    /// 扣除高级功能费用。
    async fn persist_success_and_settle(
        &self,
        task: &mut Task,
        scrape_request_dto: Option<&ScrapeRequestDto>,
        response: ScrapeResponse,
        has_screenshot: bool,
        has_proxy: bool,
    ) -> Result<bool> {
        let result_persisted = match self
            .handle_scrape_success(task, scrape_request_dto, response)
            .await
        {
            Err(e) => {
                error!("Scrape success handler failed: {}", e);
                debug!("error: {}", e);
                let err_msg = e.to_string();
                self.handle_failure(task, &err_msg).await?;
                // 结果未落库（save_result / mark_completed 失败或中途回退）
                false
            }
            Ok(false) => {
                // 丢守卫：认领方会重新执行并扣费，本 worker 不做任何计费。
                // 结果已在此前 idempotent 落库，等待方可安全收养。
                warn!(
                    "Task {} completion guard lost; feature credits not deducted by worker {}",
                    task.id, self.worker_id
                );
                true
            }
            Ok(true) => {
                debug!("Scrape success handler completed successfully");
                // 守卫胜利者扣除高级功能费用 (PRD-253)；token 费用已在
                // handle_scrape_success 内部于守卫胜利后统一扣除
                self.deduct_feature_credits(task.team_id, task.id, has_screenshot, has_proxy)
                    .await;
                true
            }
        };
        Ok(result_persisted)
    }

    /// 立即失败回填（finalize_scrape_failure 的 timeout / reason 重试上限
    /// 两分支共用）：取最新任务态 → 置 `Failed` + `completed_at` + `error`
    /// （可附额外 payload 字段）→ 守卫式写入。任务已不存在（find_by_id 空）
    /// 时静默跳过；写入失败经 `?` 上抛（与原两分支行为一致）。
    pub(super) async fn mark_task_failed_immediately(
        &self,
        task_id: crate::domain::id::Id,
        error_message: &str,
        extra_payload_fields: &[(&'static str, Value)],
    ) -> Result<()> {
        // Fetch task to ensure we have latest state
        if let Ok(Some(mut t)) = self.repository.find_by_id(task_id).await {
            t.status = TaskStatus::Failed;
            t.completed_at = Some(Utc::now());
            // 直接操作 t.payload，避免 clone 整个 JSON
            // （原实现 let mut payload = t.payload.clone + 后续 t.payload = payload
            // 在失败路径上多分配一次 JSON Value）
            if let Some(obj) = t.payload.as_object_mut() {
                obj.insert("error".to_string(), json!(error_message));
                for (key, value) in extra_payload_fields {
                    obj.insert((*key).to_string(), value.clone());
                }
            }
            // 守卫式写入：锁被抢占/任务被终结时放弃写入
            self.repository.update_task_guarded(&t).await?;
        }
        Ok(())
    }

    /// 抓取失败路径：AIMD 记账 → 重试分类（timeout 立即失败 / RetryTracker
    /// reason 上限 / handle_failure 重排）→ 失败 Webhook。
    pub(super) async fn finalize_scrape_failure(
        &self,
        task: &mut Task,
        e: EngineError,
    ) -> Result<()> {
        self.team_semaphore.record_failure(task.team_id);
        error!("Scrape failed: {}", e);
        debug!("error: {}", e);

        // 分类错误并计算重试指令（observability + reason-specific 限制）
        let retry_reason = e.retry_reason();
        let directive = RetryDirective::for_attempt(retry_reason, task.attempt_count as u32);
        debug!(
            "retry classification: reason={:?} attempt={} directive={:?}",
            retry_reason, task.attempt_count, directive
        );

        self.apply_failure_disposition(task, &e, retry_reason)
            .await?;

        // 触发失败 Webhook
        self.trigger_webhook(task, Some(e.to_string())).await;
        Ok(())
    }

    /// 按重试分类执行失败处置：timeout/expired/全引擎失败立即标记失败；
    /// reason 达上限立即标记失败（附原因 payload）；其余走重排。
    async fn apply_failure_disposition(
        &self,
        task: &mut Task,
        e: &EngineError,
        retry_reason: crate::utils::retry::RetryReason,
    ) -> Result<()> {
        // If it's a timeout error, mark as failed immediately instead of rescheduling
        let err_str = e.to_string().to_lowercase();
        if is_immediate_failure(&err_str) {
            debug!("Timeout or AllEnginesFailed detected, marking task as failed");
            self.mark_task_failed_immediately(task.id, &e.to_string(), &[])
                .await?;
        } else if retry_limit_reached(retry_reason, task.attempt_count) {
            info!(
                "retry limit reached for reason {:?} (attempt={}), \
                 marking task {} as failed",
                retry_reason, task.attempt_count, task.id
            );
            self.mark_task_failed_immediately(
                task.id,
                &e.to_string(),
                &[("retry_limit_reason", json!(format!("{:?}", retry_reason)))],
            )
            .await?;
        } else {
            let err_msg = e.to_string();
            self.handle_failure(task, &err_msg).await?;
        }
        Ok(())
    }

    // `try_coalesce` 方法已迁移至 `CoalesceCoordinator`（独立组件）。
    // 原 `request_coalescer` 字段已替换为 `coalesce_coordinator: Arc<CoalesceCoordinator>`。
    // 调用方在 `process_scrape_task` 中通过 `self.coalesce_coordinator.try_coalesce(...)` 触发。

    // process_crawl_task / handle_crawl_success / handle_crawl_failure
    // 已迁移至 `crawl_task.rs`（partial impl block）
}

/// timeout / expired / 全引擎失败 → 不重排，立即标记失败
fn is_immediate_failure(err_str: &str) -> bool {
    err_str.contains("timeout")
        || err_str.contains("expired")
        || err_str.contains("all engines failed")
}

/// reason-specific 重试上限检查
///
/// RetryTracker 各 reason 独立计数——AntiBot/FeatureToggle 达上限后
/// 即使总 max_retries 未耗尽也立即标记失败，避免无谓重试。
fn retry_limit_reached(reason: crate::utils::retry::RetryReason, attempt_count: i32) -> bool {
    let mut tracker = RetryTracker::new_default();
    for _ in 0..attempt_count {
        tracker.record(reason);
    }
    !tracker.should_retry(reason)
}
