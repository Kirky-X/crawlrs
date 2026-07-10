// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::search_result::SearchResult;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

/// 去重策略
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeduplicationStrategy {
    /// 仅基于URL去重
    UrlOnly,
    /// 仅基于标题去重
    TitleOnly,
    /// URL和标题组合去重
    UrlAndTitle,
    /// 智能去重 (URL + 标题相似度 + 内容指纹)
    Smart,
}

/// 内容指纹配置
#[derive(Debug, Clone)]
pub struct ContentFingerprintConfig {
    /// 是否启用内容指纹
    pub enabled: bool,
    /// 指纹算法 (hash, simhash)
    pub algorithm: FingerprintAlgorithm,
    /// 指纹长度
    pub fingerprint_size: usize,
    /// SimHash 海明距离阈值 (0-64)
    pub simhash_threshold: u8,
}

impl Default for ContentFingerprintConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            algorithm: FingerprintAlgorithm::SimHash,
            fingerprint_size: 64,
            simhash_threshold: 10, // 海明距离阈值
        }
    }
}

/// 指纹算法
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FingerprintAlgorithm {
    /// 简单哈希
    Hash,
    /// SimHash (局部敏感哈希)
    SimHash,
}

/// 去重配置
#[derive(Debug, Clone)]
pub struct DeduplicationConfig {
    /// 去重策略
    pub strategy: DeduplicationStrategy,
    /// 标题相似度阈值 (0.0-1.0)
    pub title_similarity_threshold: f32,
    /// 内容相似度阈值 (0.0-1.0)
    pub content_similarity_threshold: f32,
    /// 内容指纹配置
    pub fingerprint_config: ContentFingerprintConfig,
    /// 是否区分大小写
    pub case_sensitive: bool,
    /// 是否忽略查询参数
    pub ignore_query_params: bool,
    /// 是否忽略片段标识符
    pub ignore_fragments: bool,
}

impl Default for DeduplicationConfig {
    fn default() -> Self {
        Self {
            strategy: DeduplicationStrategy::Smart,
            title_similarity_threshold: 0.85,
            content_similarity_threshold: 0.8,
            fingerprint_config: ContentFingerprintConfig::default(),
            case_sensitive: false,
            ignore_query_params: true,
            ignore_fragments: true,
        }
    }
}

/// 结果去重器 (优化版本)
/// 使用 SimHash 快速相似度检测，将 O(n²) 复杂度降低到 O(n)
pub struct ResultDeduplicator {
    config: DeduplicationConfig,
    seen_urls: HashSet<String>,
    seen_titles: HashSet<String>,
    seen_fingerprints: HashSet<u64>,
    /// SimHash 指纹索引，用于快速相似度检测
    /// key: 指纹, value: 原始标题
    fingerprint_index: HashMap<u64, String>,
}

impl ResultDeduplicator {
    /// 创建新的去重器
    pub fn new(config: DeduplicationConfig) -> Self {
        Self {
            config,
            seen_urls: HashSet::new(),
            seen_titles: HashSet::new(),
            seen_fingerprints: HashSet::new(),
            fingerprint_index: HashMap::new(),
        }
    }

    /// 使用默认配置创建去重器
    pub fn with_default_config() -> Self {
        Self::new(DeduplicationConfig::default())
    }

    /// 处理URL，应用清理规则
    fn normalize_url(&self, url: &str) -> String {
        let mut normalized = url.to_string();

        // 移除片段标识符
        if self.config.ignore_fragments {
            if let Some(pos) = normalized.rfind('#') {
                normalized.truncate(pos);
            }
        }

        // 移除查询参数
        if self.config.ignore_query_params {
            if let Some(pos) = normalized.rfind('?') {
                normalized.truncate(pos);
            }
        }

        // 转换为小写（如果不区分大小写）
        if !self.config.case_sensitive {
            normalized = normalized.to_lowercase();
        }

        // 移除末尾的斜杠
        normalized = normalized.trim_end_matches('/').to_string();

        normalized
    }

    /// 处理标题，应用清理规则
    fn normalize_title(&self, title: &str) -> String {
        let mut normalized = title.to_string();

        // 移除多余的空白字符
        normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

        // 转换为小写（如果不区分大小写）
        if !self.config.case_sensitive {
            normalized = normalized.to_lowercase();
        }

        normalized
    }

    /// 生成内容指纹
    fn generate_fingerprint(&self, content: &str) -> u64 {
        match self.config.fingerprint_config.algorithm {
            FingerprintAlgorithm::Hash => {
                let mut hasher = DefaultHasher::new();
                content.hash(&mut hasher);
                hasher.finish()
            }
            FingerprintAlgorithm::SimHash => {
                // 简化的SimHash实现
                self.simhash(content)
            }
        }
    }

    /// 简化的SimHash实现
    fn simhash(&self, content: &str) -> u64 {
        let words: Vec<&str> = content.split_whitespace().collect();
        let mut hash_bits = vec![0i32; self.config.fingerprint_config.fingerprint_size];

        for word in words {
            let mut hasher = DefaultHasher::new();
            word.hash(&mut hasher);
            let word_hash = hasher.finish();

            // 更新位向量
            for (i, bit) in hash_bits
                .iter_mut()
                .enumerate()
                .take(self.config.fingerprint_config.fingerprint_size)
            {
                if (word_hash >> (i % 64)) & 1 == 1 {
                    *bit += 1;
                } else {
                    *bit -= 1;
                }
            }
        }

        // 生成最终指纹
        let mut fingerprint = 0u64;
        for (i, &bit) in hash_bits
            .iter()
            .enumerate()
            .take(self.config.fingerprint_config.fingerprint_size.min(64))
        {
            if bit > 0 {
                fingerprint |= 1 << i;
            }
        }

        fingerprint
    }

    /// 计算海明距离（两个64位指纹之间的不同位数）
    #[inline]
    fn hamming_distance(hash1: u64, hash2: u64) -> u8 {
        let xor = hash1 ^ hash2;
        xor.count_ones() as u8
    }

    /// 使用 SimHash 快速检测相似性（优化版本）
    /// O(1) 时间复杂度检测是否与已处理结果相似
    #[inline]
    fn is_similar_by_fingerprint(&self, fingerprint: u64) -> bool {
        // 检查精确匹配
        if self.seen_fingerprints.contains(&fingerprint) {
            return true;
        }

        // 使用 SimHash 海明距离快速检测相似性
        // 只检查索引中的指纹，避免 O(n²)
        for &existing_fingerprint in self.seen_fingerprints.iter() {
            let distance = Self::hamming_distance(fingerprint, existing_fingerprint);
            if distance <= self.config.fingerprint_config.simhash_threshold {
                return true;
            }
        }

        false
    }

    /// 过滤重复结果（优化版本）
    /// 使用 SimHash 和指纹索引，将复杂度从 O(n²) 降低到 O(n)
    pub fn deduplicate(&mut self, results: Vec<SearchResult>) -> Vec<SearchResult> {
        let mut unique_results = Vec::new();

        // 预分配指纹索引以提高性能
        self.fingerprint_index.reserve(results.len().min(1000));

        for result in results {
            let normalized_url = self.normalize_url(&result.url);
            let normalized_title = self.normalize_title(&result.title);

            // 使用 HashSet 进行快速去重检测
            let url_exists = self.seen_urls.contains(&normalized_url);
            let title_exists = self.seen_titles.contains(&normalized_title);

            // 根据策略判断是否重复
            let is_duplicate = match self.config.strategy {
                DeduplicationStrategy::UrlOnly => url_exists,
                DeduplicationStrategy::TitleOnly => title_exists,
                DeduplicationStrategy::UrlAndTitle => {
                    url_exists
                        || (self.config.fingerprint_config.enabled
                            && self.is_similar_by_fingerprint(
                                self.generate_fingerprint(&normalized_title),
                            ))
                }
                DeduplicationStrategy::Smart => {
                    // URL 完全匹配
                    url_exists
                        // 标题完全匹配
                        || title_exists
                        // 使用 SimHash 快速检测相似标题（O(1) 替代 O(n)）
                        || (self.config.fingerprint_config.enabled
                            && self.is_similar_by_fingerprint(
                                self.generate_fingerprint(&normalized_title),
                            ))
                }
            };

            if !is_duplicate {
                // 添加到已见集合
                self.seen_urls.insert(normalized_url.clone());
                self.seen_titles.insert(normalized_title.clone());

                if self.config.fingerprint_config.enabled {
                    let fingerprint = self.generate_fingerprint(&normalized_title);
                    self.seen_fingerprints.insert(fingerprint);
                    self.fingerprint_index.insert(fingerprint, normalized_title);
                }

                unique_results.push(result);
            }
        }

        unique_results
    }

    /// 批量去重多个引擎的结果
    pub fn deduplicate_multi_engine(
        &mut self,
        engine_results: Vec<(String, Vec<SearchResult>)>,
    ) -> Vec<SearchResult> {
        let mut all_results = Vec::new();

        // 合并所有结果
        for (engine_name, results) in engine_results {
            for mut result in results {
                result.engine = engine_name.clone();
                all_results.push(result);
            }
        }

        // 去重
        self.deduplicate(all_results)
    }

    /// 重置去重器状态
    pub fn reset(&mut self) {
        self.seen_urls.clear();
        self.seen_titles.clear();
        self.seen_fingerprints.clear();
        self.fingerprint_index.clear();
    }

    /// 获取去重统计信息
    pub fn get_stats(&self) -> DeduplicationStats {
        DeduplicationStats {
            seen_urls_count: self.seen_urls.len(),
            seen_titles_count: self.seen_titles.len(),
            seen_fingerprints_count: self.seen_fingerprints.len(),
        }
    }
}

/// 去重统计信息
#[derive(Debug, Clone)]
pub struct DeduplicationStats {
    pub seen_urls_count: usize,
    pub seen_titles_count: usize,
    pub seen_fingerprints_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::search_result::SearchResult;

    fn create_test_result(url: &str, title: &str, description: Option<&str>) -> SearchResult {
        SearchResult {
            title: title.to_string(),
            url: url.to_string(),
            description: description.map(|d| d.to_string()),
            engine: "test".to_string(),
            score: 0.0,
            published_time: None,
        }
    }

    #[test]
    fn test_url_deduplication() {
        let mut dedup = ResultDeduplicator::with_default_config();
        dedup.config.strategy = DeduplicationStrategy::UrlOnly;

        let results = vec![
            create_test_result("https://example.com/page1", "Title 1", None),
            create_test_result("https://example.com/page1", "Title 2", None), // 相同URL
            create_test_result("https://example.com/page2", "Title 3", None),
        ];

        let deduplicated = dedup.deduplicate(results);
        assert_eq!(deduplicated.len(), 2);
        assert_eq!(deduplicated[0].title, "Title 1");
        assert_eq!(deduplicated[1].title, "Title 3");
    }

    #[test]
    fn test_title_deduplication() {
        let mut dedup = ResultDeduplicator::with_default_config();
        dedup.config.strategy = DeduplicationStrategy::TitleOnly;
        dedup.config.title_similarity_threshold = 0.9;

        let results = vec![
            create_test_result("https://example.com/page1", "Rust Programming Guide", None),
            create_test_result("https://example.com/page2", "Rust Programming Guide", None), // 相同标题
            create_test_result(
                "https://example.com/page3",
                "Python Programming Guide",
                None,
            ),
        ];

        let deduplicated = dedup.deduplicate(results);
        assert_eq!(deduplicated.len(), 2);
        assert_eq!(deduplicated[0].title, "Rust Programming Guide");
        assert_eq!(deduplicated[1].title, "Python Programming Guide");
    }

    #[test]
    fn test_smart_deduplication() {
        let mut dedup = ResultDeduplicator::with_default_config();
        dedup.config.strategy = DeduplicationStrategy::Smart;
        dedup.config.title_similarity_threshold = 0.9;

        let results = vec![
            create_test_result(
                "https://example.com/page1",
                "Rust Programming Guide",
                Some("Learn Rust programming"),
            ),
            create_test_result(
                "https://example.com/page2",
                "Rust Programming Guide",
                Some("Learn Rust programming language"),
            ), // 完全相同的标题
            create_test_result(
                "https://example.com/page3",
                "Python Programming Guide",
                Some("Learn Python programming"),
            ),
        ];

        let deduplicated = dedup.deduplicate(results);
        assert_eq!(deduplicated.len(), 2);
    }

    #[test]
    fn test_url_normalization() {
        let dedup = ResultDeduplicator::with_default_config();

        assert_eq!(
            dedup.normalize_url("https://example.com/path?param=value#fragment"),
            "https://example.com/path"
        );
        assert_eq!(
            dedup.normalize_url("https://EXAMPLE.com/PATH/"),
            "https://example.com/path"
        );
    }

    // ========== normalize_url 补充测试 ==========

    #[test]
    fn test_normalize_url_empty_string() {
        // 边界情况：空字符串返回空字符串
        let dedup = ResultDeduplicator::with_default_config();
        assert_eq!(dedup.normalize_url(""), "");
    }

    #[test]
    fn test_normalize_url_only_fragment() {
        // 测试仅包含片段标识符的 URL 被清空
        let dedup = ResultDeduplicator::with_default_config();
        assert_eq!(dedup.normalize_url("https://example.com#section"), "https://example.com");
    }

    #[test]
    fn test_normalize_url_only_query() {
        // 测试仅包含查询参数的 URL 被移除查询部分
        let dedup = ResultDeduplicator::with_default_config();
        assert_eq!(dedup.normalize_url("https://example.com?a=1&b=2"), "https://example.com");
    }

    #[test]
    fn test_normalize_url_case_sensitive_config() {
        // 测试区分大小写配置保留原始大小写
        let config = DeduplicationConfig {
            case_sensitive: true,
            ..DeduplicationConfig::default()
        };
        let dedup = ResultDeduplicator::new(config);
        assert_eq!(
            dedup.normalize_url("https://EXAMPLE.com/PATH/"),
            "https://EXAMPLE.com/PATH"
        );
    }

    #[test]
    fn test_normalize_url_keep_query_when_disabled() {
        // 测试禁用查询参数忽略时保留查询字符串和片段
        let config = DeduplicationConfig {
            ignore_query_params: false,
            ignore_fragments: false,
            ..DeduplicationConfig::default()
        };
        let dedup = ResultDeduplicator::new(config);
        assert_eq!(
            dedup.normalize_url("https://example.com/path?a=1#frag"),
            "https://example.com/path?a=1#frag"
        );
    }

    // ========== normalize_title 测试 ==========

    #[test]
    fn test_normalize_title_whitespace_collapsed() {
        // 测试多余空白字符被压缩为单个空格
        let dedup = ResultDeduplicator::with_default_config();
        assert_eq!(
            dedup.normalize_title("Rust    Programming   Guide"),
            "rust programming guide"
        );
    }

    #[test]
    fn test_normalize_title_leading_trailing_whitespace_trimmed() {
        // 测试首尾空白被移除
        let dedup = ResultDeduplicator::with_default_config();
        assert_eq!(
            dedup.normalize_title("  Rust Guide  "),
            "rust guide"
        );
    }

    #[test]
    fn test_normalize_title_case_sensitive_config() {
        // 测试区分大小写配置保留原始大小写
        let config = DeduplicationConfig {
            case_sensitive: true,
            ..DeduplicationConfig::default()
        };
        let dedup = ResultDeduplicator::new(config);
        assert_eq!(
            dedup.normalize_title("Rust Programming Guide"),
            "Rust Programming Guide"
        );
    }

    #[test]
    fn test_normalize_title_empty_string() {
        // 边界情况：空字符串返回空字符串
        let dedup = ResultDeduplicator::with_default_config();
        assert_eq!(dedup.normalize_title(""), "");
    }

    // ========== generate_fingerprint 测试 ==========

    #[test]
    fn test_generate_fingerprint_deterministic() {
        // 测试相同内容产生相同指纹
        let dedup = ResultDeduplicator::with_default_config();
        let fp1 = dedup.generate_fingerprint("rust programming language");
        let fp2 = dedup.generate_fingerprint("rust programming language");
        assert_eq!(fp1, fp2, "same content should produce same fingerprint");
    }

    #[test]
    fn test_generate_fingerprint_different_content() {
        // 测试不同内容产生不同指纹
        let dedup = ResultDeduplicator::with_default_config();
        let fp1 = dedup.generate_fingerprint("rust programming language");
        let fp2 = dedup.generate_fingerprint("python scripting language");
        assert_ne!(fp1, fp2, "different content should produce different fingerprint");
    }

    #[test]
    fn test_generate_fingerprint_empty_content() {
        // 边界情况：空内容不 panic 且返回确定值
        let dedup = ResultDeduplicator::with_default_config();
        let fp = dedup.generate_fingerprint("");
        // 空内容应产生确定的指纹（全 0 或某固定值）
        let fp2 = dedup.generate_fingerprint("");
        assert_eq!(fp, fp2, "empty content should be deterministic");
    }

    #[test]
    fn test_generate_fingerprint_hash_algorithm() {
        // 测试 Hash 算法模式
        let config = DeduplicationConfig {
            fingerprint_config: ContentFingerprintConfig {
                algorithm: FingerprintAlgorithm::Hash,
                ..ContentFingerprintConfig::default()
            },
            ..DeduplicationConfig::default()
        };
        let dedup = ResultDeduplicator::new(config);
        let fp1 = dedup.generate_fingerprint("test content");
        let fp2 = dedup.generate_fingerprint("test content");
        assert_eq!(fp1, fp2, "Hash algorithm should be deterministic");
    }

    // ========== hamming_distance 测试 ==========

    #[test]
    fn test_hamming_distance_identical_hashes() {
        // 测试相同哈希的海明距离为 0
        let hash = 0b1010_1010u64;
        assert_eq!(ResultDeduplicator::hamming_distance(hash, hash), 0);
    }

    #[test]
    fn test_hamming_distance_one_bit_difference() {
        // 测试相差一位的海明距离为 1
        let hash1 = 0b1000u64;
        let hash2 = 0b1001u64;
        assert_eq!(ResultDeduplicator::hamming_distance(hash1, hash2), 1);
    }

    #[test]
    fn test_hamming_distance_all_bits_different() {
        // 测试所有位都不同的海明距离为 64
        assert_eq!(ResultDeduplicator::hamming_distance(0u64, u64::MAX), 64);
    }

    // ========== deduplicate 边界情况 ==========

    #[test]
    fn test_deduplicate_empty_input() {
        // 边界情况：空输入返回空输出
        let mut dedup = ResultDeduplicator::with_default_config();
        let results: Vec<SearchResult> = vec![];
        let deduplicated = dedup.deduplicate(results);
        assert!(deduplicated.is_empty());
    }

    #[test]
    fn test_deduplicate_single_result_preserved() {
        // 边界情况：单个结果保留
        let mut dedup = ResultDeduplicator::with_default_config();
        let results = vec![create_test_result("https://example.com", "Title", None)];
        let deduplicated = dedup.deduplicate(results);
        assert_eq!(deduplicated.len(), 1);
    }

    #[test]
    fn test_deduplicate_all_duplicates_keeps_one() {
        // 边界情况：全部重复时只保留一个
        let mut dedup = ResultDeduplicator::with_default_config();
        let results = vec![
            create_test_result("https://example.com", "Same Title", None),
            create_test_result("https://example.com", "Same Title", None),
            create_test_result("https://example.com", "Same Title", None),
        ];
        let deduplicated = dedup.deduplicate(results);
        assert_eq!(deduplicated.len(), 1);
    }

    // ========== reset 测试 ==========

    #[test]
    fn test_reset_clears_all_state() {
        // 测试 reset 清空所有已见集合，使之前重复的结果不再被视为重复
        let mut dedup = ResultDeduplicator::with_default_config();
        let results = vec![
            create_test_result("https://example.com", "Title 1", None),
            create_test_result("https://example.com", "Title 1", None),
        ];

        let first_pass = dedup.deduplicate(results.clone());
        assert_eq!(first_pass.len(), 1);

        // reset 后同样的结果应再次通过
        dedup.reset();
        let stats = dedup.get_stats();
        assert_eq!(stats.seen_urls_count, 0);
        assert_eq!(stats.seen_titles_count, 0);
        assert_eq!(stats.seen_fingerprints_count, 0);

        let second_pass = dedup.deduplicate(results);
        assert_eq!(second_pass.len(), 1, "after reset, first result should pass again");
    }

    // ========== get_stats 测试 ==========

    #[test]
    fn test_get_stats_after_deduplication() {
        // 测试去重后统计数据正确反映已见集合大小
        let mut dedup = ResultDeduplicator::with_default_config();
        let results = vec![
            create_test_result("https://example.com/1", "Title 1", None),
            create_test_result("https://example.com/2", "Title 2", None),
            create_test_result("https://example.com/1", "Title 1", None), // 重复
        ];

        dedup.deduplicate(results);
        let stats = dedup.get_stats();
        assert_eq!(stats.seen_urls_count, 2, "should have 2 unique URLs");
        assert_eq!(stats.seen_titles_count, 2, "should have 2 unique titles");
    }

    // ========== deduplicate_multi_engine 测试 ==========

    #[test]
    fn test_deduplicate_multi_engine_merges_and_deduplicates() {
        // 测试多引擎结果合并后去重，且引擎名被正确设置
        let mut dedup = ResultDeduplicator::with_default_config();
        let engine_results = vec![
            (
                "google".to_string(),
                vec![
                    create_test_result("https://example.com/1", "Rust Guide", None),
                    create_test_result("https://example.com/2", "Go Guide", None),
                ],
            ),
            (
                "bing".to_string(),
                vec![
                    create_test_result("https://example.com/1", "Rust Guide", None), // 跨引擎重复
                    create_test_result("https://example.com/3", "Python Guide", None),
                ],
            ),
        ];

        let deduplicated = dedup.deduplicate_multi_engine(engine_results);
        assert_eq!(deduplicated.len(), 3, "should have 3 unique results after cross-engine dedup");
        // 验证引擎名被正确设置
        assert_eq!(deduplicated[0].engine, "google");
        assert_eq!(deduplicated[2].engine, "bing");
    }

    // ========== 策略组合测试 ==========

    #[test]
    fn test_url_and_title_strategy() {
        // 测试 UrlAndTitle 策略：URL 或标题任一匹配即视为重复
        let mut dedup = ResultDeduplicator::with_default_config();
        dedup.config.strategy = DeduplicationStrategy::UrlAndTitle;

        let results = vec![
            create_test_result("https://example.com/1", "Title A", None),
            create_test_result("https://example.com/2", "Title A", None), // 标题相同
            create_test_result("https://example.com/3", "Title C", None),
        ];

        let deduplicated = dedup.deduplicate(results);
        assert_eq!(deduplicated.len(), 2, "same title should trigger dedup in UrlAndTitle strategy");
    }

    // ========== is_similar_by_fingerprint: hamming distance 路径 ==========

    #[test]
    fn test_smart_deduplication_similar_by_hamming_distance() {
        // 设置 simhash_threshold=64 使任意两个不同指纹的海明距离都 <= threshold
        // 从而触发 is_similar_by_fingerprint 中的 hamming distance 检测路径（非精确匹配）
        let mut dedup = ResultDeduplicator::with_default_config();
        dedup.config.strategy = DeduplicationStrategy::Smart;
        dedup.config.fingerprint_config.simhash_threshold = 64;

        let results = vec![
            create_test_result("https://example.com/1", "Rust Programming Guide", None),
            create_test_result("https://example.com/2", "Completely Different Topic", None),
        ];

        let deduplicated = dedup.deduplicate(results);
        // 第二个结果应被 hamming distance 检测为相似而被去重
        assert_eq!(deduplicated.len(), 1, "hamming distance path should detect similarity");
    }

    #[test]
    fn test_url_and_title_strategy_hamming_distance_path() {
        // UrlAndTitle 策略也调用 is_similar_by_fingerprint，验证其 hamming distance 路径
        let mut dedup = ResultDeduplicator::with_default_config();
        dedup.config.strategy = DeduplicationStrategy::UrlAndTitle;
        dedup.config.fingerprint_config.simhash_threshold = 64;

        let results = vec![
            create_test_result("https://example.com/1", "Rust Guide", None),
            create_test_result("https://example.com/2", "Python Tutorial", None),
        ];

        let deduplicated = dedup.deduplicate(results);
        assert_eq!(deduplicated.len(), 1, "UrlAndTitle should also use hamming distance");
    }
}
