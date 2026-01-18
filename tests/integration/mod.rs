// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(feature = "engine-fire-cdp")]
pub mod api;
#[cfg(feature = "engine-fire-cdp")]
pub mod api_tests;
#[cfg(feature = "engine-playwright")]
pub mod browser_tests;
// pub mod crawl_service_test;
// #[cfg(feature = "testing-integration-tests")]
// pub mod extract_credit_deduction_test;
#[cfg(feature = "engine-fire-cdp")]
pub mod google_tests;
pub mod health_check;
pub mod health_monitor_test;
pub mod helpers;
// #[cfg(feature = "testing-integration-tests")]
// pub mod optimized_tests;
// #[cfg(feature = "testing-integration-tests")]
// pub mod page_interactions_test;
pub mod queue_client_test;
// #[cfg(feature = "testing-integration-tests")]
// pub mod real_components_test;
// pub mod real_interactions_test;
#[cfg(any(feature = "engine-fire-cdp", feature = "engine-playwright"))]
pub mod real_world_test;
pub mod repositories;
pub mod s3_storage_test;
// #[cfg(feature = "testing-integration-tests")]
// pub mod scheduler_test;
pub mod scrape_handler_test;
#[cfg(feature = "search-google")]
pub mod search_engines_test;
pub mod search_uat_test;
// #[cfg(feature = "testing-integration-tests")]
// pub mod uat_scenarios_test;
#[cfg(feature = "engine-fire-cdp")]
pub mod verify_google_routing;
// pub mod webhook_test;
// #[cfg(feature = "testing-integration-tests")]
// pub mod worker_tests;
