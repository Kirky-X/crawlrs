use crawlrs::infrastructure::cache::redis_client::RedisClient;
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let redis_url = "redis://127.0.0.1:6379";
    let redis = RedisClient::new(redis_url).await.unwrap();
    let redis = Arc::new(redis);

    let api_key = "test-api-key-123";

    // Set rate limit config
    let key1 = format!("rate_limit_config:{}", api_key);
    let value1 = json!({"requests_per_minute": 1, "capacity": 1}).to_string();
    println!("Setting key: {} = {}", key1, value1);
    let _: () = redis.set(&key1, &value1, 60).await.unwrap();

    // Also set with prefix
    let key2 = format!("crawlrs:ratelimit:config:{}", api_key);
    let value2 = json!({"requests_per_minute": 1, "capacity": 1}).to_string();
    println!("Setting key: {} = {}", key2, value2);
    let _: () = redis.set(&key2, &value2, 60).await.unwrap();

    // Verify
    let keys: Vec<String> = redis.keys("*rate_limit*").await.unwrap();
    println!("All rate_limit keys: {:?}", keys);

    for key in &keys {
        let value: String = redis.get(key).await.unwrap();
        println!("  {} = {}", key, value);
    }

    // Try get with fallback
    let result1: Option<String> = redis.get(&key1).await.ok().flatten();
    println!("\nDirect get {}: {:?}", key1, result1);

    let result2: Option<String> = redis.get(&key2).await.ok().flatten();
    println!("Direct get {}: {:?}", key2, result2);
}
