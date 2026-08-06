    use super::*;
    use crate::engines::client::reqwest::ReqwestEngine;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicU64;

    #[tokio::test]
    async fn test_engine_router_creation() {
        let http_client = Arc::new(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
        );
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(ReqwestEngine::new(http_client))];
        let router = EngineRouter::new(engines);

        assert_eq!(router.strategy, LoadBalancingStrategy::SmartHybrid);
    }

    #[tokio::test]
    async fn test_route_respects_max_engine_attempts() {
        struct CountingEngine {
            name: &'static str,
            calls: Arc<std::sync::atomic::AtomicU32>,
            ok: bool,
        }

        #[async_trait]
        impl ScraperEngine for CountingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if self.ok {
                    Ok(InternalScrapeResponse {
                        status_code: 200,
                        content: "ok".to_string(),
                        screenshot: None,
                        content_type: "text/html".to_string(),
                        headers: HashMap::new(),
                        response_time_ms: 10,
                    })
                } else {
                    Err(EngineError::Timeout(Duration::from_millis(10)))
                }
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                self.name
            }
        }

        let c1 = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c2 = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c3 = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let e1: Arc<dyn ScraperEngine> = Arc::new(CountingEngine {
            name: "e1",
            calls: c1.clone(),
            ok: false,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(CountingEngine {
            name: "e2",
            calls: c2.clone(),
            ok: false,
        });
        let e3: Arc<dyn ScraperEngine> = Arc::new(CountingEngine {
            name: "e3",
            calls: c3.clone(),
            ok: true,
        });

        let mut router = EngineRouter::new(vec![e1, e2, e3]);
        router.set_strategy(LoadBalancingStrategy::RoundRobin);
        router.set_max_engine_attempts(2);

        let request = InternalScrapeRequest {
            url: "http://1.1.1.1".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
            needs_mllm: false,
        };
        let result = router.route(&request).await;

        assert!(result.is_err());
        assert_eq!(c1.load(Ordering::SeqCst), 1);
        assert_eq!(c2.load(Ordering::SeqCst), 1);
        assert_eq!(c3.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_engine_score_calculation() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![];
        let router = EngineRouter::new(engines);

        let stats = EngineStats {
            success_rate: 0.9,
            avg_response_time: Duration::from_millis(200),
            usage_count: 10,
            last_used: None,
        };

        let score = router.calculate_engine_score(1.0, &stats);
        assert!(score > 0.8 && score <= 1.0);
    }

    // === Mock engine with controllable support score ===

    struct MockEngine {
        engine_name: &'static str,
        score: u8,
    }

    #[async_trait]
    impl ScraperEngine for MockEngine {
        async fn scrape(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            Ok(InternalScrapeResponse {
                status_code: 200,
                // T013：内容需 ≥200 字节且可见文本 ≥50 字符，
                // 否则被 antibot::classify Step 5 误判为 near-empty structural block。
                content: "<html><body><h1>Mock Response</h1><p>This is a mock response for testing router logic. It contains enough visible text to avoid being flagged as a near-empty shell by the antibot classifier.</p></body></html>".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 10,
            })
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            self.score
        }

        fn name(&self) -> &'static str {
            self.engine_name
        }
    }

    fn make_request() -> InternalScrapeRequest {
        InternalScrapeRequest {
            url: "http://example.com".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
            needs_mllm: false,
        }
    }

    // === should_filter_by_feature tests ===

    #[test]
    fn test_should_filter_by_feature_screenshot_low_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "low-score",
            score: 30,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_screenshot = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_some());
        assert!(result.unwrap().contains("screenshots"));
    }

    #[test]
    fn test_should_filter_by_feature_screenshot_high_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "high-score",
            score: 80,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_screenshot = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_none());
    }

    #[test]
    fn test_should_filter_by_feature_js_low_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "low-score",
            score: 20,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_js = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_some());
        assert!(result.unwrap().contains("JavaScript"));
    }

    #[test]
    fn test_should_filter_by_feature_actions_low_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "low-score",
            score: 10,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.actions = vec![crate::engines::engine_client::InternalPageAction::Click {
            selector: "#btn".to_string(),
        }];
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_some());
        assert!(result.unwrap().contains("JavaScript"));
    }

    #[test]
    fn test_should_filter_by_feature_tls_fingerprint_low_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "low-score",
            score: 40,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_tls_fingerprint = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_some());
        assert!(result.unwrap().contains("TLS fingerprinting"));
    }

    #[test]
    fn test_should_filter_by_feature_tls_fingerprint_high_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "high-score",
            score: 60,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_tls_fingerprint = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_none());
    }

    #[test]
    fn test_should_filter_by_feature_no_special_needs() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "any",
            score: 10,
        });
        let router = EngineRouter::new(vec![]);
        let request = make_request();
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_none());
    }

    // === sort_candidates_by_strategy tests ===

    #[test]
    fn test_sort_round_robin_preserves_order() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::RoundRobin);
        let stats = std::collections::HashMap::new();
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (1.0, router.engines[0].clone()),
            (2.0, router.engines[1].clone()),
        ];
        let original_names: Vec<_> = candidates.iter().map(|(_, e)| e.name()).collect();
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        let sorted_names: Vec<_> = candidates.iter().map(|(_, e)| e.name()).collect();
        assert_eq!(original_names, sorted_names);
    }

    #[test]
    fn test_sort_weighted_round_robin_by_score() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
        let stats = std::collections::HashMap::new();
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (0.5, router.engines[0].clone()),
            (0.9, router.engines[1].clone()),
        ];
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        assert_eq!(candidates[0].1.name(), "e2");
        assert_eq!(candidates[1].1.name(), "e1");
    }

    #[test]
    fn test_sort_least_connections_by_usage() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::LeastConnections);
        let mut stats = std::collections::HashMap::new();
        stats.insert(
            "e1".to_string(),
            EngineStats {
                usage_count: 100,
                ..Default::default()
            },
        );
        stats.insert(
            "e2".to_string(),
            EngineStats {
                usage_count: 5,
                ..Default::default()
            },
        );
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (1.0, router.engines[0].clone()),
            (1.0, router.engines[1].clone()),
        ];
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        assert_eq!(candidates[0].1.name(), "e2");
        assert_eq!(candidates[1].1.name(), "e1");
    }

    #[test]
    fn test_sort_fastest_response_by_time() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::FastestResponse);
        let mut stats = std::collections::HashMap::new();
        stats.insert(
            "e1".to_string(),
            EngineStats {
                avg_response_time: Duration::from_millis(500),
                ..Default::default()
            },
        );
        stats.insert(
            "e2".to_string(),
            EngineStats {
                avg_response_time: Duration::from_millis(100),
                ..Default::default()
            },
        );
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (1.0, router.engines[0].clone()),
            (1.0, router.engines[1].clone()),
        ];
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        assert_eq!(candidates[0].1.name(), "e2");
    }

    #[test]
    fn test_sort_random_shuffles() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockEngine {
                engine_name: "e1",
                score: 100,
            }),
            Arc::new(MockEngine {
                engine_name: "e2",
                score: 100,
            }),
            Arc::new(MockEngine {
                engine_name: "e3",
                score: 100,
            }),
        ];
        let mut router = EngineRouter::new(engines);
        router.set_strategy(LoadBalancingStrategy::Random);
        let stats = std::collections::HashMap::new();
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> =
            router.engines.iter().map(|e| (1.0, e.clone())).collect();
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        // Random may or may not change order, just verify no panic
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn test_sort_smart_hybrid_combined() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::SmartHybrid);
        let mut stats = std::collections::HashMap::new();
        stats.insert(
            "e1".to_string(),
            EngineStats {
                success_rate: 0.5,
                avg_response_time: Duration::from_millis(800),
                usage_count: 50,
                last_used: None,
            },
        );
        stats.insert(
            "e2".to_string(),
            EngineStats {
                success_rate: 0.95,
                avg_response_time: Duration::from_millis(100),
                usage_count: 5,
                last_used: None,
            },
        );
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (0.6, router.engines[0].clone()),
            (0.9, router.engines[1].clone()),
        ];
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        assert_eq!(candidates[0].1.name(), "e2");
    }

    // === update_engine_stats tests ===

    #[test]
    fn test_update_engine_stats_success() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        router.update_engine_stats("test", true, Duration::from_millis(100));
        let stats = router.get_engine_stats();
        let stat = stats.get("test").unwrap();
        assert!(stat.success_rate > 0.9);
        assert_eq!(stat.usage_count, 1);
        assert!(stat.last_used.is_some());
    }

    #[test]
    fn test_update_engine_stats_failure() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        router.update_engine_stats("test", false, Duration::from_millis(500));
        let stats = router.get_engine_stats();
        let stat = stats.get("test").unwrap();
        assert!(stat.success_rate < 1.0);
        assert_eq!(stat.usage_count, 1);
    }

    #[test]
    fn test_update_engine_stats_nonexistent() {
        let router = EngineRouter::new(vec![]);
        router.update_engine_stats("nonexistent", true, Duration::from_millis(50));
        // Should not panic
    }

    // === get_next_round_robin_index tests ===

    #[test]
    fn test_get_next_round_robin_index_wraps() {
        let router = EngineRouter::new(vec![]);
        let idx1 = router.get_next_round_robin_index(3);
        let idx2 = router.get_next_round_robin_index(3);
        let idx3 = router.get_next_round_robin_index(3);
        let idx4 = router.get_next_round_robin_index(3);
        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(idx3, 2);
        assert_eq!(idx4, 0);
    }

    #[test]
    fn test_get_next_round_robin_index_single() {
        let router = EngineRouter::new(vec![]);
        let idx = router.get_next_round_robin_index(1);
        assert_eq!(idx, 0);
    }

    // === reset_engine_stats tests ===

    #[test]
    fn test_reset_engine_stats() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        router.update_engine_stats("test", false, Duration::from_millis(500));
        let stats_before = router.get_engine_stats();
        assert_eq!(stats_before.get("test").unwrap().usage_count, 1);
        router.reset_engine_stats("test");
        let stats_after = router.get_engine_stats();
        let stat = stats_after.get("test").unwrap();
        assert_eq!(stat.usage_count, 0);
        assert_eq!(stat.success_rate, 1.0);
    }

    #[test]
    fn test_reset_engine_stats_nonexistent() {
        let router = EngineRouter::new(vec![]);
        router.reset_engine_stats("nonexistent");
        // Should not panic
    }

    // === register_engine tests ===

    #[test]
    fn test_register_engine() {
        let mut router = EngineRouter::new(vec![]);
        assert!(router.get_engine_stats().is_empty());
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "new-engine",
            score: 100,
        });
        router.register_engine(engine);
        assert!(router.get_engine_stats().contains_key("new-engine"));
        assert_eq!(router.registered_engines(), vec!["new-engine".to_string()]);
    }

    // === RouterMetrics tests ===

    #[test]
    fn test_router_metrics_new() {
        let metrics = RouterMetrics::new();
        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.successful_requests.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.failed_requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_router_metrics_record_candidates() {
        let metrics = RouterMetrics::new();
        metrics.record_candidates(5);
        metrics.record_candidates(3);
        assert_eq!(metrics.candidate_count_total.load(Ordering::Relaxed), 8);
    }

    #[test]
    fn test_router_metrics_record_attempt() {
        let metrics = RouterMetrics::new();
        metrics.record_attempt();
        metrics.record_attempt();
        metrics.record_attempt();
        assert_eq!(metrics.attempt_count_total.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_router_metrics_record_engine_selection() {
        let metrics = RouterMetrics::new();
        metrics.record_engine_selection("engine1");
        assert_eq!(metrics.engine_selection_total.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_router_metrics_record_engine_latency() {
        let metrics = RouterMetrics::new();
        // 架构审查 HIGH-1 修复后：record_engine_latency 自带 entry().or_insert() 自动初始化，
        // 不再依赖 record_engine_selection 预初始化 latencies=0
        metrics.record_engine_latency("engine1", Duration::from_millis(100));
        metrics.record_engine_latency("engine1", Duration::from_millis(200));
        // 累计延迟应为 100+200=300ms = 300_000_000ns
        // PERF-004: AtomicU64 load 读取
        let total_ref = metrics.engine_latencies.get("engine1").unwrap();
        assert_eq!(total_ref.load(Ordering::Relaxed), 300_000_000);
        // avg 需要 success_count 同步存在（get_avg_latency_ns 检查两者）
        // 单独记录 latency 不更新 success_count，故 avg 仍为 None
        let avg = metrics.get_avg_latency_ns("engine1");
        assert!(avg.is_none());
    }

    #[test]
    fn test_router_metrics_record_engine_success() {
        let metrics = RouterMetrics::new();
        // 架构审查 HIGH-1 修复后：record_engine_success 自带 entry().or_insert() 自动初始化，
        // 不再需要测试手动 insert 0 预初始化
        metrics.record_engine_success("engine1");
        metrics.record_engine_success("engine1");
        let count_ref = metrics.engine_success_count.get("engine1").unwrap();
        assert_eq!(count_ref.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_router_metrics_record_engine_failure() {
        let metrics = RouterMetrics::new();
        // 架构审查 HIGH-1 修复后：record_engine_failure 自带 entry().or_insert() 自动初始化
        // failure_count 和 failure_classification 都不再需要测试手动 insert 0 预初始化
        metrics.record_engine_failure("engine1", "timeout error");
        metrics.record_engine_failure("engine1", "network error");
        let count_ref = metrics.engine_failure_count.get("engine1").unwrap();
        assert_eq!(count_ref.load(Ordering::Relaxed), 2);
        let timeout_count = metrics.failure_classification.get("timeout").unwrap();
        assert_eq!(*timeout_count, 1);
        let network_count = metrics.failure_classification.get("network_error").unwrap();
        assert_eq!(*network_count, 1);
    }

    #[test]
    fn test_router_metrics_classify_error() {
        assert_eq!(RouterMetrics::classify_error("request timeout"), "timeout");
        assert_eq!(
            RouterMetrics::classify_error("SSRF protection triggered"),
            "ssrf_protection"
        );
        assert_eq!(
            RouterMetrics::classify_error("network unreachable"),
            "network_error"
        );
        assert_eq!(
            RouterMetrics::classify_error("circuit breaker open"),
            "circuit_breaker"
        );
        assert_eq!(
            RouterMetrics::classify_error("browser crashed"),
            "browser_error"
        );
        assert_eq!(RouterMetrics::classify_error("unknown issue"), "other");
    }

    #[test]
    fn test_router_metrics_get_success_rate() {
        let metrics = RouterMetrics::new();
        assert_eq!(metrics.get_success_rate(), 1.0);
        metrics.total_requests.store(10, Ordering::Relaxed);
        metrics.successful_requests.store(7, Ordering::Relaxed);
        assert_eq!(metrics.get_success_rate(), 0.7);
    }

    #[test]
    fn test_router_metrics_get_avg_latency_ns_no_data() {
        let metrics = RouterMetrics::new();
        assert!(metrics.get_avg_latency_ns("nonexistent").is_none());
    }

    #[test]
    fn test_router_metrics_get_avg_latency_ns_with_data() {
        let metrics = RouterMetrics::new();
        // Manually populate both latencies and success_count
        metrics
            .engine_latencies
            .insert("engine1".to_string(), AtomicU64::new(1_000_000));
        metrics
            .engine_success_count
            .insert("engine1".to_string(), AtomicU64::new(10));
        let avg = metrics.get_avg_latency_ns("engine1");
        assert_eq!(avg, Some(100_000));
    }

    #[test]
    fn test_router_metrics_record_engine_success_initializes_to_one() {
        // Verify that record_engine_success self-initializes the counter to 1
        // when key doesn't exist (架构审查 HIGH-1 修复：原实现 noop when key missing
        // 导致 success_count 永远为 0，与"成功必须被计数"的业务语义冲突)。
        let metrics = RouterMetrics::new();
        metrics.record_engine_success("engine1");
        assert_eq!(
            metrics
                .engine_success_count
                .get("engine1")
                .unwrap()
                .load(Ordering::Relaxed),
            1u64
        );

        // 二次调用应递增，不应重置
        metrics.record_engine_success("engine1");
        assert_eq!(
            metrics
                .engine_success_count
                .get("engine1")
                .unwrap()
                .load(Ordering::Relaxed),
            2u64
        );
    }

    // === calculate_engine_score edge cases ===

    #[test]
    fn test_calculate_engine_score_zero_success_rate() {
        let router = EngineRouter::new(vec![]);
        let stats = EngineStats {
            success_rate: 0.0,
            avg_response_time: Duration::from_secs(5),
            usage_count: 500,
            last_used: None,
        };
        let score = router.calculate_engine_score(1.0, &stats);
        assert!(score < 0.5);
    }

    #[test]
    fn test_calculate_engine_score_perfect_stats() {
        let router = EngineRouter::new(vec![]);
        let stats = EngineStats {
            success_rate: 1.0,
            avg_response_time: Duration::from_millis(10),
            usage_count: 0,
            last_used: None,
        };
        let score = router.calculate_engine_score(1.0, &stats);
        assert!(score > 0.95);
    }

    #[test]
    fn test_calculate_engine_score_high_usage_penalty() {
        let router = EngineRouter::new(vec![]);
        let stats = EngineStats {
            success_rate: 1.0,
            avg_response_time: Duration::from_millis(10),
            usage_count: 2000,
            last_used: None,
        };
        let score = router.calculate_engine_score(1.0, &stats);
        let perfect_stats = EngineStats {
            success_rate: 1.0,
            avg_response_time: Duration::from_millis(10),
            usage_count: 0,
            last_used: None,
        };
        let perfect_score = router.calculate_engine_score(1.0, &perfect_stats);
        assert!(score < perfect_score);
    }

    // === Setter tests ===

    #[test]
    fn test_set_max_engine_attempts() {
        let mut router = EngineRouter::new(vec![]);
        router.set_max_engine_attempts(5);
        assert_eq!(router.max_engine_attempts, 5);
    }

    #[test]
    fn test_set_max_engine_attempts_min_one() {
        let mut router = EngineRouter::new(vec![]);
        router.set_max_engine_attempts(0);
        assert_eq!(router.max_engine_attempts, 1);
    }

    #[test]
    fn test_set_max_retries() {
        let mut router = EngineRouter::new(vec![]);
        router.set_max_retries(10);
        assert_eq!(router.max_retries, 10);
    }

    #[test]
    fn test_set_max_retries_min_one() {
        let mut router = EngineRouter::new(vec![]);
        router.set_max_retries(0);
        assert_eq!(router.max_retries, 1);
    }

    #[test]
    fn test_set_feature_filter_enabled() {
        let mut router = EngineRouter::new(vec![]);
        router.set_feature_filter_enabled(false);
        assert!(!router.feature_filter_enabled);
        router.set_feature_filter_enabled(true);
        assert!(router.feature_filter_enabled);
    }

    #[test]
    fn test_set_race_mode_enabled() {
        let mut router = EngineRouter::new(vec![]);
        router.set_race_mode_enabled(true);
        assert!(router.race_mode_enabled);
    }

    #[test]
    fn test_set_dynamic_threshold_factor() {
        let mut router = EngineRouter::new(vec![]);
        router.set_dynamic_threshold_factor(1.5);
        assert_eq!(router.dynamic_threshold_factor, 1.5);
    }

    #[test]
    fn test_set_dynamic_threshold_factor_clamped() {
        let mut router = EngineRouter::new(vec![]);
        router.set_dynamic_threshold_factor(0.01);
        assert_eq!(router.dynamic_threshold_factor, 0.1);
        router.set_dynamic_threshold_factor(3.0);
        assert_eq!(router.dynamic_threshold_factor, 2.0);
    }

    #[test]
    fn test_set_strategy() {
        let mut router = EngineRouter::new(vec![]);
        router.set_strategy(LoadBalancingStrategy::RoundRobin);
        assert_eq!(router.strategy, LoadBalancingStrategy::RoundRobin);
        router.set_strategy(LoadBalancingStrategy::Random);
        assert_eq!(router.strategy, LoadBalancingStrategy::Random);
    }

    #[test]
    fn test_metrics_accessor() {
        let router = EngineRouter::new(vec![]);
        let _metrics = router.metrics();
    }

    // === with_circuit_breaker_and_strategy constructor test ===

    #[test]
    fn test_with_circuit_breaker_and_strategy() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let cb = Arc::new(CircuitBreaker::new());
        let router = EngineRouter::with_circuit_breaker_and_strategy(
            vec![engine],
            cb,
            LoadBalancingStrategy::LeastConnections,
        );
        assert_eq!(router.strategy, LoadBalancingStrategy::LeastConnections);
        assert!(router.get_engine_stats().contains_key("test"));
    }

    // === get_engines test ===

    #[test]
    fn test_get_engines() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let router = EngineRouter::new(vec![e1, e2]);
        let engines = router.get_engines();
        assert_eq!(engines.len(), 2);
        assert_eq!(engines[0].name(), "e1");
        assert_eq!(engines[1].name(), "e2");
    }

    // === EngineStats default test ===

    #[test]
    fn test_engine_stats_default() {
        let stats = EngineStats::default();
        assert_eq!(stats.success_rate, 1.0);
        assert_eq!(stats.avg_response_time, Duration::from_millis(500));
        assert!(stats.last_used.is_none());
        assert_eq!(stats.usage_count, 0);
    }

    // === 路由成功路径测试 ===

    #[tokio::test]
    async fn test_route_success_path() {
        // 测试路由成功路径：MockEngine 返回成功响应
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "success-engine",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status_code, 200);
        assert!(response.content.contains("Mock Response"));
    }

    // === SSRF 保护测试 ===

    #[tokio::test]
    async fn test_route_ssrf_protection() {
        // 测试 SSRF 保护：使用内部 IP 地址应被拒绝
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        let mut request = make_request();
        request.url = "http://127.0.0.1".to_string();
        let result = router.route(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::SsrfProtection(_)));
    }

    // === 不可重试错误测试 ===

    #[tokio::test]
    async fn test_route_non_retryable_error() {
        // 测试不可重试错误：引擎返回 InvalidUrl 时应立即失败
        struct NonRetryableEngine;
        #[async_trait]
        impl ScraperEngine for NonRetryableEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::InvalidUrl("bad url".to_string()))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "non-retryable"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(NonRetryableEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::InvalidUrl(_)));
    }

    // === 最大重试次数测试 ===

    #[tokio::test]
    async fn test_route_max_retries_reached() {
        // 测试最大重试次数：所有引擎都返回可重试错误，应达到最大重试次数后失败
        struct AlwaysTimeoutEngine;
        #[async_trait]
        impl ScraperEngine for AlwaysTimeoutEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::Timeout(Duration::from_secs(10)))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "always-timeout"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(AlwaysTimeoutEngine);
        let mut router = EngineRouter::new(vec![engine]);
        router.set_max_retries(1);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::Timeout(_)));
    }

    // === 竞速模式测试 ===

    #[tokio::test]
    async fn test_route_race_mode_success() {
        // 测试竞速模式：多个引擎并发，返回最快的成功结果
        struct FastEngine {
            name: &'static str,
            delay_ms: u64,
        }
        #[async_trait]
        impl ScraperEngine for FastEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: format!("from-{}", self.name),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: self.delay_ms,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                self.name
            }
        }
        let e1: Arc<dyn ScraperEngine> = Arc::new(FastEngine {
            name: "slow",
            delay_ms: 500,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(FastEngine {
            name: "fast",
            delay_ms: 10,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.content.starts_with("from-"));
    }

    /// T070/§17：验证 race 胜出后延迟被记录到 hedge_controller
    #[tokio::test]
    async fn test_route_race_mode_records_hedge_latency() {
        struct FastEngine {
            name: &'static str,
            delay_ms: u64,
        }
        #[async_trait]
        impl ScraperEngine for FastEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: format!("from-{}", self.name),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: self.delay_ms,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                self.name
            }
        }
        let e1: Arc<dyn ScraperEngine> = Arc::new(FastEngine {
            name: "slow",
            delay_ms: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(FastEngine {
            name: "fast",
            delay_ms: 5,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);

        // 初始：hedge 样本为 0
        assert_eq!(router.hedge_controller().sample_count(), 0);

        // 多次 race 胜出 fast（5ms）
        let request = make_request();
        for _ in 0..12 {
            let _ = router.route(&request).await.unwrap();
        }

        // 12 次 race 后：hedge 样本数应 ≥ DEFAULT_MIN_SAMPLES（10）
        let controller = router.hedge_controller();
        assert!(
            controller.sample_count() >= 10,
            "hedge should have >= 10 samples, got {}",
            controller.sample_count()
        );

        // P84 阈值应可用（fast 5ms + slow 100ms 但 race 总是 fast 胜）
        let threshold = controller
            .p84_threshold()
            .expect("P84 threshold should be available");
        // fast 总是胜出，延迟应近 5ms（容忍调度抖动）
        let threshold_ms = threshold.as_secs_f64() * 1000.0;
        assert!(
            threshold_ms < 50.0,
            "P84 should be near fast engine latency, got {threshold_ms}ms"
        );

        // 已耗时大于阈值：should_hedge 应为 true
        assert!(router
            .hedge_controller()
            .should_hedge(Duration::from_millis(100)));
        // 已耗时小于阈值：should_hedge 应为 false
        assert!(!router
            .hedge_controller()
            .should_hedge(Duration::from_micros(1)));
    }

    #[tokio::test]
    async fn test_route_race_mode_all_fail() {
        // 测试竞速模式：所有引擎都失败时返回错误
        struct FailingEngine;
        #[async_trait]
        impl ScraperEngine for FailingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::RequestFailed("connection refused".to_string()))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "failing"
            }
        }
        let e1: Arc<dyn ScraperEngine> = Arc::new(FailingEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(FailingEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
    }

    // === 聚合测试 ===

    #[tokio::test]
    async fn test_aggregate_no_candidates() {
        // 测试聚合：所有引擎 support_score 为 0，候选列表为空
        struct ZeroScoreEngine;
        #[async_trait]
        impl ScraperEngine for ZeroScoreEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                0
            }
            fn name(&self) -> &'static str {
                "zero-score"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(ZeroScoreEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();
        let result = router.aggregate(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::AllEnginesFailed(_)));
    }

    #[tokio::test]
    async fn test_aggregate_all_engines_fail() {
        // 测试聚合：所有引擎都失败
        struct FailingEngine;
        #[async_trait]
        impl ScraperEngine for FailingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::RequestFailed("failed".to_string()))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "failing"
            }
        }
        let e1: Arc<dyn ScraperEngine> = Arc::new(FailingEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(FailingEngine);
        let router = EngineRouter::new(vec![e1, e2]);
        let request = make_request();
        let result = router.aggregate(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::AllEnginesFailed(_)));
    }

    // === EngineRouterTrait 通过 trait 对象测试 ===

    #[tokio::test]
    async fn test_engine_router_trait_methods() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "trait-test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        let trait_ref: &dyn EngineRouterTrait = &router;

        // 测试 registered_engines
        let engines = trait_ref.registered_engines();
        assert_eq!(engines, vec!["trait-test".to_string()]);

        // 测试 get_engine_stats
        let stats = trait_ref.get_engine_stats();
        assert!(stats.contains_key("trait-test"));

        // 测试 reset_engine_stats
        trait_ref.reset_engine_stats("trait-test");
        let stats_after = trait_ref.get_engine_stats();
        assert_eq!(stats_after.get("trait-test").unwrap().usage_count, 0);

        // 测试 route 通过 trait
        let request = make_request();
        let result = trait_ref.route(&request).await;
        assert!(result.is_ok());
    }

    // === select_optimal_engines 边界情况 ===

    #[tokio::test]
    async fn test_route_no_engines_available() {
        // 测试没有引擎时返回 AllEnginesFailed
        let router = EngineRouter::new(vec![]);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::AllEnginesFailed(_)));
    }

    #[tokio::test]
    async fn test_route_support_score_zero_filtered() {
        // 测试 support_score 为 0 的引擎被过滤
        struct ZeroScoreEngine;
        #[async_trait]
        impl ScraperEngine for ZeroScoreEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                0
            }
            fn name(&self) -> &'static str {
                "zero-score"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(ZeroScoreEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::AllEnginesFailed(_)
        ));
    }

    // === EngineRouterTrait method coverage ===
    // These tests call methods through the trait interface (not the public
    // wrapper methods) to cover the trait impl at lines 1028-1055.

    #[tokio::test]
    async fn test_trait_aggregate_delegates_to_impl() {
        struct SucceedingEngine;
        #[async_trait]
        impl ScraperEngine for SucceedingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    // T013：同 MockEngine，需 ≥200 字节可见文本避免 antibot 误判
                    content: "<html><body><h1>OK</h1><p>Succeeding engine response for testing trait delegation. It has enough visible text to pass the antibot classifier near-empty check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "succeeding"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(SucceedingEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();

        // Call through the trait, not the public wrapper method
        let result = EngineRouterTrait::aggregate(&router, &request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_trait_route_delegates_to_impl() {
        // Also cover the trait route method (line 1030-1034)
        struct SucceedingEngine;
        #[async_trait]
        impl ScraperEngine for SucceedingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    // T013：同 MockEngine，需 ≥200 字节可见文本避免 antibot 误判
                    content: "<html><body><h1>OK</h1><p>Succeeding engine response for testing trait delegation. It has enough visible text to pass the antibot classifier near-empty check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "succeeding"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(SucceedingEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();

        let result = EngineRouterTrait::route(&router, &request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_feature_filter_excludes_low_score_engine_for_screenshot() {
        // Cover the feature_filter_enabled branch (line 407-411):
        // When needs_screenshot=true and engine support_score < 50,
        // the engine should be filtered out.
        struct LowScoreEngine;
        #[async_trait]
        impl ScraperEngine for LowScoreEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                10 // Below the 50 threshold
            }
            fn name(&self) -> &'static str {
                "low-score"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(LowScoreEngine);
        let mut router = EngineRouter::new(vec![engine]);
        // feature_filter_enabled defaults to true, but set explicitly for clarity
        router.set_feature_filter_enabled(true);

        let request = InternalScrapeRequest {
            url: "http://example.com".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: true, // This triggers the feature filter
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
            needs_mllm: false,
        };

        // The low-score engine should be filtered out, leaving no candidates
        let result = router.route(&request).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::AllEnginesFailed(_)
        ));
    }

    // === circuit breaker open branch (line 402-404) ===

    #[tokio::test]
    async fn test_route_skips_engine_when_circuit_breaker_open() {
        // Cover line 403: when the circuit breaker for an engine is open,
        // select_optimal_engines should `continue` past it, leaving no
        // candidates and producing AllEnginesFailed.
        use crate::engines::circuit_breaker::CircuitConfig;

        struct CountingEngine {
            name: &'static str,
            calls: Arc<std::sync::atomic::AtomicU32>,
        }

        #[async_trait]
        impl ScraperEngine for CountingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 1,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                self.name
            }
        }

        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let engine: Arc<dyn ScraperEngine> = Arc::new(CountingEngine {
            name: "guarded",
            calls: calls.clone(),
        });
        let router = EngineRouter::new(vec![engine]);

        // Force the circuit breaker open for this engine: a config with
        // failure_threshold = 1 plus a single recorded failure flips it to
        // Open immediately.
        router.circuit_breaker.set_config(
            "guarded",
            CircuitConfig {
                failure_threshold: 1,
                recovery_timeout: Duration::from_secs(60),
                failure_window: Duration::from_secs(60),
            },
        );
        router.circuit_breaker.record_failure("guarded");
        assert!(
            router.circuit_breaker.is_open("guarded"),
            "circuit breaker should be open after 1 failure"
        );

        let request = make_request();
        let result = router.route(&request).await;
        assert!(
            result.is_err(),
            "route should fail when the only engine is open"
        );
        assert!(
            matches!(result.unwrap_err(), EngineError::AllEnginesFailed(_)),
            "expected AllEnginesFailed when circuit breaker blocks the only engine"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "engine must not be invoked when its circuit breaker is open"
        );
    }

    // === route_internal remaining=0 branch (line 690-692) ===

    #[tokio::test]
    async fn test_route_returns_timeout_when_remaining_time_zero() {
        // Cover line 691: after one engine attempt burns the full request
        // timeout, the next iteration computes `remaining = 0` and short-
        // circuits with EngineError::Timeout.
        struct SlowTimeoutEngine;
        #[async_trait]
        impl ScraperEngine for SlowTimeoutEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                // Sleep longer than the request timeout so that, after this
                // attempt, `start_time.elapsed()` exceeds `request.timeout`.
                tokio::time::sleep(Duration::from_millis(120)).await;
                Err(EngineError::Timeout(Duration::from_millis(120)))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "slow-timeout"
            }
        }

        let e1: Arc<dyn ScraperEngine> = Arc::new(SlowTimeoutEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(SlowTimeoutEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        // Need at least 2 attempts so the loop iterates after the first
        // failure; otherwise the first Timeout would propagate directly.
        router.set_max_engine_attempts(2);
        router.set_max_retries(5);

        let mut request = make_request();
        request.timeout = Duration::from_millis(10);

        let result = router.route(&request).await;
        assert!(result.is_err(), "route should fail with Timeout");
        match result.unwrap_err() {
            EngineError::Timeout(d) => {
                assert_eq!(
                    d,
                    Duration::from_millis(10),
                    "should report the original request timeout"
                );
            }
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    // === T062: MRT 瀑布式超时测试（red → green） ===
    //
    // design.md §14 / T062：router 顺序 fallback 路径用 `min(remaining, engine.mrt())`
    // 包裹单引擎调用，超 MRT 即切下一引擎（瀑布式），不切整体失败。
    // race_mode 路径不受影响（保留作为可选模式）。

    /// T062 red：engine1 的 scrape() 耗时超过其 MRT → router 应通过 tokio::time::timeout
    /// 在 MRT 时刻取消 engine1，记录 Timeout 失败，瀑布式切到 engine2 → engine2 立即成功。
    ///
    /// 未实现 T062 时：engine1 直接 sleep 500ms 后返回 Ok，engine2 永远不会被调用，
    /// 总耗时 ~500ms，测试失败（断言 engine2_called=true 与 elapsed<400ms）。
    #[tokio::test]
    async fn test_route_mrt_waterfall_first_engine_exceeds_mrt_falls_to_second() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Instant;

        let engine1_called = Arc::new(AtomicBool::new(false));
        let engine2_called = Arc::new(AtomicBool::new(false));

        /// MRT 短但 scrape 耗时长的引擎（用于触发 MRT 超时）
        struct MrtSlowEngine {
            mrt: Duration,
            sleep_dur: Duration,
            called: Arc<AtomicBool>,
        }
        #[async_trait]
        impl ScraperEngine for MrtSlowEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.called.store(true, Ordering::SeqCst);
                // 模拟引擎处理耗时超过 MRT
                tokio::time::sleep(self.sleep_dur).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "<html><body><h1>Slow Engine Response</h1><p>This is the slow engine response with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: self.sleep_dur.as_millis() as u64,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100 // 高分 → 优先被选中
            }
            fn name(&self) -> &'static str {
                "mrt-slow"
            }
            fn max_response_time(&self) -> Duration {
                self.mrt
            }
        }

        /// 立即返回成功的引擎（作为 fallback 目标）
        struct FastEngine {
            called: Arc<AtomicBool>,
        }
        #[async_trait]
        impl ScraperEngine for FastEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.called.store(true, Ordering::SeqCst);
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "<html><body><h1>Fast Engine Response</h1><p>This is the fast engine response with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 1,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                90 // 较低分 → 作为 fallback
            }
            fn name(&self) -> &'static str {
                "fast"
            }
        }

        // engine1: MRT=50ms, scrape sleeps 500ms（远超 MRT）
        let e1: Arc<dyn ScraperEngine> = Arc::new(MrtSlowEngine {
            mrt: Duration::from_millis(50),
            sleep_dur: Duration::from_millis(500),
            called: engine1_called.clone(),
        });
        // engine2: 立即返回成功
        let e2: Arc<dyn ScraperEngine> = Arc::new(FastEngine {
            called: engine2_called.clone(),
        });

        let mut router = EngineRouter::new(vec![e1, e2]);
        // 允许至少 2 次引擎尝试（瀑布式 fallback）
        router.set_max_engine_attempts(2);
        router.set_max_retries(5);
        // 关闭 race_mode 与特征过滤，确保走顺序 fallback 路径
        router.set_race_mode_enabled(false);

        let request = make_request();
        let start = Instant::now();
        let result = router.route(&request).await;
        let elapsed = start.elapsed();

        // 断言 1：最终成功（通过 engine2）
        assert!(result.is_ok(), "route should succeed via engine2 fallback");
        let resp = result.unwrap();
        assert_eq!(
            resp.content, "<html><body><h1>Fast Engine Response</h1><p>This is the fast engine response with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>",
            "response should come from fast engine (waterfall fallback)"
        );

        // 断言 2：engine1 被调用（首次尝试）
        assert!(
            engine1_called.load(Ordering::SeqCst),
            "engine1 should have been called (first attempt)"
        );
        // 断言 3：engine2 也被调用（MRT 超时后瀑布式切换）
        assert!(
            engine2_called.load(Ordering::SeqCst),
            "engine2 should have been called after engine1 exceeded MRT (waterfall)"
        );

        // 断言 4：总耗时应远小于 engine1 的 500ms sleep
        // （MRT=50ms + engine2 ~1ms + 开销，应 < 400ms）
        assert!(
            elapsed < Duration::from_millis(400),
            "should not wait for engine1's full 500ms sleep; elapsed={:?}",
            elapsed
        );
    }

    /// T062 red：engine 在其 MRT 内完成 → router 不应误超时，直接返回成功。
    ///
    /// 这是一个回归保护测试：确保 MRT 包裹不会破坏正常行为。
    /// 即使未实现 T062，此测试也应通过（因为 engine1 直接返回 Ok）。
    #[tokio::test]
    async fn test_route_mrt_engine_within_mrt_succeeds_normally() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let engine1_called = Arc::new(AtomicBool::new(false));

        struct MrtOkEngine {
            mrt: Duration,
            sleep_dur: Duration,
            called: Arc<AtomicBool>,
        }
        #[async_trait]
        impl ScraperEngine for MrtOkEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.called.store(true, Ordering::SeqCst);
                tokio::time::sleep(self.sleep_dur).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "<html><body><h1>Real Content Page</h1><p>This is a real page with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: self.sleep_dur.as_millis() as u64,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "mrt-ok"
            }
            fn max_response_time(&self) -> Duration {
                self.mrt
            }
        }

        // engine1: MRT=1s, scrape sleeps 50ms（在 MRT 内）
        let e1: Arc<dyn ScraperEngine> = Arc::new(MrtOkEngine {
            mrt: Duration::from_secs(1),
            sleep_dur: Duration::from_millis(50),
            called: engine1_called.clone(),
        });

        let mut router = EngineRouter::new(vec![e1]);
        router.set_race_mode_enabled(false);

        let request = make_request();
        let result = router.route(&request).await;

        assert!(result.is_ok(), "engine within MRT should succeed");
        let resp = result.unwrap();
        assert_eq!(resp.content, "<html><body><h1>Real Content Page</h1><p>This is a real page with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>");
        assert!(
            engine1_called.load(Ordering::SeqCst),
            "engine1 should have been called"
        );
    }

    /// T062 red：当 remaining < mrt 时，router 应使用 remaining 作为超时
    /// （即请求整体超时优先于单引擎 MRT）。
    ///
    /// 场景：request.timeout=80ms, engine.mrt=10s
    /// engine1 sleep 200ms → 应在 ~80ms 时被取消（remaining 耗尽），返回 Timeout。
    #[tokio::test]
    async fn test_route_mrt_uses_min_remaining_when_remaining_less_than_mrt() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let engine1_calls = Arc::new(AtomicU32::new(0));

        struct LongMrtSlowEngine {
            mrt: Duration,
            sleep_dur: Duration,
            calls: Arc<AtomicU32>,
        }
        #[async_trait]
        impl ScraperEngine for LongMrtSlowEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(self.sleep_dur).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "should-not-reach-here".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 0,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "long-mrt-slow"
            }
            fn max_response_time(&self) -> Duration {
                self.mrt
            }
        }

        // engine1: MRT=10s（很长），但 request.timeout=80ms（很短）
        // engine1 sleep 200ms → 应在 ~80ms 时被 remaining 超时取消
        let e1: Arc<dyn ScraperEngine> = Arc::new(LongMrtSlowEngine {
            mrt: Duration::from_secs(10),
            sleep_dur: Duration::from_millis(200),
            calls: engine1_calls.clone(),
        });

        let mut router = EngineRouter::new(vec![e1]);
        router.set_max_engine_attempts(1);
        router.set_max_retries(1);
        router.set_race_mode_enabled(false);

        let mut request = make_request();
        request.timeout = Duration::from_millis(80);

        let start = std::time::Instant::now();
        let result = router.route(&request).await;
        let elapsed = start.elapsed();

        assert!(result.is_err(), "should fail with Timeout");
        match result.unwrap_err() {
            EngineError::Timeout(_) => {}
            other => panic!("Expected Timeout, got {:?}", other),
        }
        // 总耗时应 ~80ms（remaining 耗尽），不是 200ms（engine sleep）或 10s（MRT）。
        // 阈值放宽到 2000ms 容忍 CI/容器环境下 tokio 调度抖动（实测容器中可能 500ms+）。
        assert!(
            elapsed < Duration::from_millis(2000),
            "should timeout at ~80ms (remaining); elapsed={:?}",
            elapsed
        );
        assert_eq!(
            engine1_calls.load(Ordering::SeqCst),
            1,
            "engine1 should be called exactly once"
        );
    }

    // === route_race_mode remaining=0 branch (line 796-798) ===

    #[tokio::test]
    async fn test_route_race_mode_returns_timeout_when_remaining_zero() {
        // Cover line 797: when race_mode is enabled and `remaining` is
        // already zero by the time route_race_mode is entered, the function
        // should immediately return EngineError::Timeout(request.timeout).
        struct NeverCalledEngine;
        #[async_trait]
        impl ScraperEngine for NeverCalledEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                panic!("engine must not be called when remaining time is zero");
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "never-called"
            }
        }

        let e1: Arc<dyn ScraperEngine> = Arc::new(NeverCalledEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(NeverCalledEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);

        let mut request = make_request();
        // Zero timeout forces `remaining = 0` immediately inside
        // route_race_mode, before any engine future is polled.
        request.timeout = Duration::from_millis(0);

        let result = router.route(&request).await;
        assert!(result.is_err(), "race_mode with zero remaining should fail");
        match result.unwrap_err() {
            EngineError::Timeout(_) => {}
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    // === route_race_mode non-retryable error branch (line 884-886) ===

    #[tokio::test]
    async fn test_route_race_mode_non_retryable_error_returns_err() {
        // Cover line 885: when the first race future resolves to a non-
        // retryable error, route_race_mode should return that error as-is
        // instead of recording a circuit-breaker failure.
        struct InvalidUrlEngine;
        #[async_trait]
        impl ScraperEngine for InvalidUrlEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::InvalidUrl("malformed url".to_string()))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "invalid-url"
            }
        }

        let e1: Arc<dyn ScraperEngine> = Arc::new(InvalidUrlEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(InvalidUrlEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);

        let request = make_request();
        let result = router.route(&request).await;
        assert!(
            result.is_err(),
            "race_mode with non-retryable error should fail"
        );
        match result.unwrap_err() {
            EngineError::InvalidUrl(msg) => {
                assert_eq!(msg, "malformed url");
            }
            other => panic!("Expected InvalidUrl, got {:?}", other),
        }
    }

    // === route_race_mode select_all timeout branch (line 890-897) ===

    #[tokio::test]
    async fn test_route_race_mode_returns_timeout_on_select_all_timeout() {
        // Cover lines 892 & 896: when every racing engine takes longer than
        // `timeout_duration` to resolve, time::timeout fires the Err(_)
        // branch and route_race_mode returns EngineError::Timeout with the
        // timeout_duration it actually waited.
        struct SlowOkEngine;
        #[async_trait]
        impl ScraperEngine for SlowOkEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                // Sleep much longer than the race timeout window so that
                // select_all never resolves in time.
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 5000,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "slow-ok"
            }
        }

        let e1: Arc<dyn ScraperEngine> = Arc::new(SlowOkEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(SlowOkEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);

        let mut request = make_request();
        // Pick a request.timeout that is comfortably larger than the time
        // route_internal spends before entering route_race_mode (so that
        // `remaining` is non-zero and we don't hit the early-return at
        // line 797), but smaller than the 5s engine sleep so that
        // time::timeout fires the Err(_) branch.
        // timeout_duration = remaining.max(100ms) ≈ 1s here.
        request.timeout = Duration::from_secs(1);

        let result = router.route(&request).await;
        assert!(
            result.is_err(),
            "race_mode with all-slow engines should time out"
        );
        match result.unwrap_err() {
            EngineError::Timeout(d) => {
                // timeout_duration = max(remaining, 100ms). Since
                // request.timeout = 1s and elapsed before route_race_mode
                // is negligible, d should be ~1s, and at minimum 100ms.
                assert!(
                    d >= Duration::from_millis(100),
                    "timeout duration should be at least 100ms, got {:?}",
                    d
                );
            }
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    // === T066: sort_candidates_by_strategy 不同策略排序测试 ===

    #[test]
    fn test_sort_candidates_fastest_response() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "slow",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "fast",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1.clone(), e2.clone()]);
        router.set_strategy(LoadBalancingStrategy::FastestResponse);

        // 预设引擎统计：slow=2s, fast=50ms
        router
            .engine_stats
            .get_mut("slow")
            .unwrap()
            .avg_response_time = Duration::from_secs(2);
        router
            .engine_stats
            .get_mut("fast")
            .unwrap()
            .avg_response_time = Duration::from_millis(50);

        let stats: std::collections::HashMap<String, EngineStats> = router
            .engine_stats
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect();

        let mut candidates = vec![(1.0, e1), (1.0, e2)];
        router.sort_candidates_by_strategy(&mut candidates, &stats);

        assert_eq!(
            candidates[0].1.name(),
            "fast",
            "FastestResponse should put fastest engine first"
        );
        assert_eq!(candidates[1].1.name(), "slow");
    }

    #[test]
    fn test_sort_candidates_least_connections() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "busy",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "idle",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1.clone(), e2.clone()]);
        router.set_strategy(LoadBalancingStrategy::LeastConnections);

        // 预设使用次数：busy=500, idle=5
        router.engine_stats.get_mut("busy").unwrap().usage_count = 500;
        router.engine_stats.get_mut("idle").unwrap().usage_count = 5;

        let stats: std::collections::HashMap<String, EngineStats> = router
            .engine_stats
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect();

        let mut candidates = vec![(1.0, e1), (1.0, e2)];
        router.sort_candidates_by_strategy(&mut candidates, &stats);

        assert_eq!(
            candidates[0].1.name(),
            "idle",
            "LeastConnections should put least-used engine first"
        );
        assert_eq!(candidates[1].1.name(), "busy");
    }

    #[test]
    fn test_sort_candidates_smart_hybrid_score_priority() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "low-score",
            score: 50,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "high-score",
            score: 100,
        });
        let router = EngineRouter::new(vec![e1.clone(), e2.clone()]);
        // Default strategy is SmartHybrid

        let stats: std::collections::HashMap<String, EngineStats> = router
            .engine_stats
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect();

        let mut candidates = vec![(0.5, e1), (1.0, e2)];
        router.sort_candidates_by_strategy(&mut candidates, &stats);

        assert_eq!(
            candidates[0].1.name(),
            "high-score",
            "SmartHybrid should put higher-scored engine first"
        );
    }

    // === T066: select_optimal_engines 熔断器过滤测试 ===

    #[test]
    fn test_select_optimal_engines_circuit_breaker_skips_open() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "open-cb",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "closed-cb",
            score: 100,
        });
        let router = EngineRouter::new(vec![e1.clone(), e2.clone()]);

        // 打开 open-cb 的熔断器
        for _ in 0..10 {
            router.circuit_breaker.record_failure("open-cb");
        }

        let request = make_request();
        let candidates = router.select_optimal_engines(&request);

        let names: Vec<&str> = candidates.iter().map(|(_, e)| e.name()).collect();
        assert!(
            !names.contains(&"open-cb"),
            "circuit breaker open engine should be filtered out"
        );
        assert!(
            names.contains(&"closed-cb"),
            "closed circuit breaker engine should remain"
        );
    }

    #[test]
    fn test_select_optimal_engines_feature_filter_tls_fingerprint() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "no-tls",
            score: 10,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "with-tls",
            score: 100,
        });
        let router = EngineRouter::new(vec![e1.clone(), e2.clone()]);

        let mut request = make_request();
        request.needs_tls_fingerprint = true;

        let candidates = router.select_optimal_engines(&request);
        let names: Vec<&str> = candidates.iter().map(|(_, e)| e.name()).collect();

        assert!(
            !names.contains(&"no-tls"),
            "low-score engine should be filtered when needs_tls_fingerprint=true"
        );
        assert!(
            names.contains(&"with-tls"),
            "high-score engine should remain"
        );
    }
