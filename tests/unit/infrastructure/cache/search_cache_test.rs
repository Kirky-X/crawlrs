// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::time::Duration;
use tokio::time::sleep;
use crawlrs::infrastructure::cache::cache_manager::CacheManager;
use crawlrs::infrastructure::cache::cache_strategy::CacheStrategyConfig;
use crawlrs::domain::models::search_result::SearchResult;

#[tokio::test]
async fn test_cache_key_generation() {
    let cache = CacheManager::new(CacheStrategyConfig::default(), None).await.unwrap();
    
    let query1 = "rust programming";
    let query2 = "rust programming";
    let query3 = "python programming";
    
    // Then: 相同查询生成相同的键
    let key1 = CacheManager::generate_cache_key(query1, 10, Some("en"), Some("US"), None);
    let key2 = CacheManager::generate_cache_key(query2, 10, Some("en"), Some("US"), None);
    assert_eq!(key1, key2);
    
    // Then: 不同查询生成不同的键
    let key3 = CacheManager::generate_cache_key(query3, 10, Some("en"), Some("US"), None);
    assert_ne!(key1, key3);
}

#[tokio::test]
async fn test_cache_set_and_get() {
    let cache = CacheManager::new(CacheStrategyConfig::default(), None).await.unwrap();
    
    let key = "search:v1:test123";
    let results = vec![
        SearchResult {
            title: "Test Result".to_string(),
            url: "https://example.com".to_string(),
            description: Some("Test description".to_string()),
            engine: "google".to_string(),
            score: 0.9,
            published_time: None,
        }
    ];
    
    // When: 写入缓存
    cache.set(key, results.clone(), Some(Duration::from_secs(60))).await.unwrap();
    
    // Then: 可以读取
    let cached = cache.get(key).await.unwrap();
    assert!(cached.is_some());
    let cached_results = cached.unwrap();
    assert_eq!(cached_results.len(), 1);
    assert_eq!(cached_results[0].title, "Test Result");
}

#[tokio::test]
async fn test_cache_expiration() {
    let cache = CacheManager::new(CacheStrategyConfig::default(), None).await.unwrap();
    
    let key = "search:v1:expire_test";
    let results = vec![SearchResult {
        title: "Test".to_string(),
        url: "https://example.com".to_string(),
        description: None,
        engine: "test".to_string(),
        score: 0.5,
        published_time: None,
    }];
    
    // When: 设置 1 秒 TTL
    cache.set(key, results, Some(Duration::from_secs(1))).await.unwrap();
    
    // Then: 立即可读
    assert!(cache.get(key).await.unwrap().is_some());
    
    // When: 等待 2 秒
    sleep(Duration::from_secs(2)).await;
    
    // Then: 缓存已过期
    assert!(cache.get(key).await.unwrap().is_none());
}

#[tokio::test]
async fn test_cache_key_differentiation() {
    let cache = CacheManager::new(CacheStrategyConfig::default(), None).await.unwrap();
    
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
    
    // When: 不同参数生成不同的键
    let key1 = CacheManager::generate_cache_key(base_query, 10, Some("en"), Some("US"), Some("google"));
    let key2 = CacheManager::generate_cache_key(base_query, 10, Some("en"), Some("US"), Some("bing"));
    
    // Then: 设置不同的缓存值
    cache.set(&key1, results1, Some(Duration::from_secs(60))).await.unwrap();
    cache.set(&key2, results2, Some(Duration::from_secs(60))).await.unwrap();
    
    // Then: 获取时应该得到不同的结果
    let cached1 = cache.get(&key1).await.unwrap().unwrap();
    let cached2 = cache.get(&key2).await.unwrap().unwrap();
    
    assert_eq!(cached1[0].engine, "google");
    assert_eq!(cached2[0].engine, "bing");
}

#[tokio::test]
async fn test_cache_batch_operations() {
    let cache = CacheManager::new(CacheStrategyConfig::default(), None).await.unwrap();
    
    let entries = vec![
        ("key1".to_string(), vec![SearchResult {
            title: "Result 1".to_string(),
            url: "https://example1.com".to_string(),
            description: None,
            engine: "google".to_string(),
            score: 0.8,
            published_time: None,
        }]),
        ("key2".to_string(), vec![SearchResult {
            title: "Result 2".to_string(),
            url: "https://example2.com".to_string(),
            description: None,
            engine: "bing".to_string(),
            score: 0.7,
            published_time: None,
        }]),
    ];
    
    // When: 批量设置
    cache.set_batch(entries.clone(), Some(Duration::from_secs(60))).await.unwrap();
    
    // Then: 批量获取
    let keys = vec!["key1".to_string(), "key2".to_string()];
    let results = cache.get_batch(&keys).await.unwrap();
    
    assert_eq!(results.len(), 2);
    assert!(results[0].is_some());
    assert!(results[1].is_some());
    assert_eq!(results[0].as_ref().unwrap()[0].title, "Result 1");
    assert_eq!(results[1].as_ref().unwrap()[0].title, "Result 2");
}