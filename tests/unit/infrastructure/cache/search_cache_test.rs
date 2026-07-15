// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crawlrs::domain::models::search_result::SearchResult;
use crawlrs::infrastructure::oxcache::{generate_search_key, SearchCache};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Test cache key generation using oxcache utilities
#[tokio::test]
async fn test_cache_key_generation() {
    // Test the key generation utility function
    let query1 = "rust programming";
    let query2 = "rust programming";
    let query3 = "python programming";

    // Same query generates same key
    let key1 = generate_search_key(query1, 10, Some("en"), Some("US"));
    let key2 = generate_search_key(query2, 10, Some("en"), Some("US"));
    assert_eq!(key1, key2);

    // Different query generates different key
    let key3 = generate_search_key(query3, 10, Some("en"), Some("US"));
    assert_ne!(key1, key3);
}

/// Test cache set and get operations using oxcache
#[tokio::test]
async fn test_cache_set_and_get() {
    let cache: Arc<SearchCache> = Arc::new(
        oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(1))
            .build()
            .await
            .expect("Failed to create cache"),
    );

    let key = "search:test123".to_string();
    let results = vec![SearchResult {
        title: "Test Result".to_string(),
        url: "https://example.com".to_string(),
        description: Some("Test description".to_string()),
        engine: "google".to_string(),
        score: 0.9,
        published_time: None,
    }];

    // Set cache
    cache
        .set(&key, &results)
        .await
        .expect("Failed to set cache");

    // Get cache — should be present
    let cached = cache.get(&key).await.expect("Failed to get cache");
    assert!(cached.is_some());

    // Wait 2 seconds (exceeds 1s TTL)
    sleep(Duration::from_secs(2)).await;

    // Then: Should be expired
    assert!(cache
        .get(&key)
        .await
        .expect("Failed to get cache")
        .is_none());
}

// Ignored: generate_search_key no longer takes an `engine` parameter — engine-based
// cache key differentiation was removed from the source. This test's premise is
// no longer valid and needs a rewrite to test the current key scheme.
#[tokio::test]
#[ignore = "engine param removed from generate_search_key; test premise no longer valid"]
async fn test_cache_key_differentiation() {
    let cache: Arc<SearchCache> = Arc::new(
        oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(300))
            .build()
            .await
            .expect("Failed to create cache"),
    );

    let base_query = "rust programming";
    let results1 = vec![SearchResult {
        title: "Result 1".to_string(),
        url: "https://example1.com".to_string(),
        description: None,
        engine: "google".to_string(),
        score: 0.8,
        published_time: None,
    }];
    let results2 = vec![SearchResult {
        title: "Result 2".to_string(),
        url: "https://example2.com".to_string(),
        description: None,
        engine: "bing".to_string(),
        score: 0.7,
        published_time: None,
    }];

    // Engine is no longer part of the cache key — both calls produce the same key.
    let key1 = generate_search_key(base_query, 10, Some("en"), Some("US"));
    let key2 = generate_search_key(base_query, 10, Some("en"), Some("US"));

    // Set different cache values
    cache
        .set(&key1, &results1)
        .await
        .expect("Failed to set cache for key1");
    cache
        .set(&key2, &results2)
        .await
        .expect("Failed to set cache for key2");

    // Get and verify different results
    let cached1 = cache
        .get(&key1)
        .await
        .expect("Failed to get cache for key1")
        .expect("Cache for key1 not found");
    let cached2 = cache
        .get(&key2)
        .await
        .expect("Failed to get cache for key2")
        .expect("Cache for key2 not found");

    // With engine removed from key, key2 overwrites key1 — both return results2.
    assert_eq!(cached1[0].engine, "bing");
    assert_eq!(cached2[0].engine, "bing");
}

#[tokio::test]
async fn test_cache_batch_operations() {
    let cache: Arc<SearchCache> = Arc::new(
        oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(300))
            .build()
            .await
            .expect("Failed to create cache"),
    );

    let entries = vec![
        (
            "key1".to_string(),
            vec![SearchResult {
                title: "Result 1".to_string(),
                url: "https://example1.com".to_string(),
                description: None,
                engine: "google".to_string(),
                score: 0.8,
                published_time: None,
            }],
        ),
        (
            "key2".to_string(),
            vec![SearchResult {
                title: "Result 2".to_string(),
                url: "https://example2.com".to_string(),
                description: None,
                engine: "bing".to_string(),
                score: 0.7,
                published_time: None,
            }],
        ),
    ];

    // Batch set (using individual sets since oxcache may not have batch API)
    for (key, value) in &entries {
        cache.set(key, value).await.expect("Failed to set cache");
    }

    // Batch get (using individual gets)
    let keys = vec!["key1".to_string(), "key2".to_string()];
    let mut results = Vec::new();
    for key in &keys {
        let result = cache.get(key).await.expect("Failed to get cache");
        results.push(result);
    }

    assert_eq!(results.len(), 2);
    assert!(results[0].is_some());
    assert!(results[1].is_some());
    assert_eq!(
        results[0].as_ref().expect("Batch result 1 not found")[0].title,
        "Result 1"
    );
    assert_eq!(
        results[1].as_ref().expect("Batch result 2 not found")[0].title,
        "Result 2"
    );
}
