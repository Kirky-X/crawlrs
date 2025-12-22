// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::entities::search_result::SearchResult;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

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
}

impl Default for ContentFingerprintConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            algorithm: FingerprintAlgorithm::SimHash,
            fingerprint_size: 64,
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

/// 结果去重器
pub struct ResultDeduplicator {
    config: DeduplicationConfig,
    seen_urls: HashSet<String>,
    seen_titles: HashSet<String>,
    seen_fingerprints: HashSet<u64>,
}

impl ResultDeduplicator {
    /// 创建新的去重器
    pub fn new(config: DeduplicationConfig) -> Self {
        Self {
            config,
            seen_urls: HashSet::new(),
            seen_titles: HashSet::new(),
            seen_fingerprints: HashSet::new(),
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
            for i in 0..self.config.fingerprint_config.fingerprint_size {
                if (word_hash >> (i % 64)) & 1 == 1 {
                    hash_bits[i] += 1;
                } else {
                    hash_bits[i] -= 1;
                }
            }
        }

        // 生成最终指纹
        let mut fingerprint = 0u64;
        for i in 0..self.config.fingerprint_config.fingerprint_size.min(64) {
            if hash_bits[i] > 0 {
                fingerprint |= 1 << i;
            }
        }

        fingerprint
    }

    /// 计算两个字符串的相似度（使用Jaro-Winkler）
    fn calculate_similarity(&self, s1: &str, s2: &str) -> f32 {
        strsim::jaro_winkler(s1, s2) as f32
    }

    /// 检查两个结果是否重复
    fn is_duplicate(&self, result: &SearchResult, existing: &SearchResult) -> bool {
        match self.config.strategy {
            DeduplicationStrategy::UrlOnly => {
                let url1 = self.normalize_url(&result.url);
                let url2 = self.normalize_url(&existing.url);
                url1 == url2
            }
            DeduplicationStrategy::TitleOnly => {
                let title1 = self.normalize_title(&result.title);
                let title2 = self.normalize_title(&existing.title);
                self.calculate_similarity(&title1, &title2) > self.config.title_similarity_threshold
            }
            DeduplicationStrategy::UrlAndTitle => {
                let url1 = self.normalize_url(&result.url);
                let url2 = self.normalize_url(&existing.url);
                let title1 = self.normalize_title(&result.title);
                let title2 = self.normalize_title(&existing.title);
                
                url1 == url2 || self.calculate_similarity(&title1, &title2) > self.config.title_similarity_threshold
            }
            DeduplicationStrategy::Smart => {
                self.is_smart_duplicate(result, existing)
            }
        }
    }

    /// 智能去重检查
    fn is_smart_duplicate(&self, result: &SearchResult, existing: &SearchResult) -> bool {
        // URL检查
        let url1 = self.normalize_url(&result.url);
        let url2 = self.normalize_url(&existing.url);
        if url1 == url2 {
            return true;
        }

        // 标题相似度检查
        let title1 = self.normalize_title(&result.title);
        let title2 = self.normalize_title(&existing.title);
        let title_similarity = self.calculate_similarity(&title1, &title2);
        
        if title_similarity > self.config.title_similarity_threshold {
            return true;
        }

        // 内容指纹检查（如果有内容）
        if self.config.fingerprint_config.enabled {
            if let (Some(content1), Some(content2)) = (&result.description, &existing.description) {
                let fingerprint1 = self.generate_fingerprint(content1);
                let fingerprint2 = self.generate_fingerprint(content2);
                
                // 计算指纹相似度（汉明距离）
                let fingerprint_similarity = self.calculate_fingerprint_similarity(fingerprint1, fingerprint2);
                
                if fingerprint_similarity > self.config.content_similarity_threshold {
                    return true;
                }
            }
        }

        false
    }

    /// 计算指纹相似度（基于汉明距离）
    fn calculate_fingerprint_similarity(&self, fp1: u64, fp2: u64) -> f32 {
        let hamming_distance = (fp1 ^ fp2).count_ones() as f32;
        let max_distance = 64.0;
        1.0 - (hamming_distance / max_distance)
    }

    /// 过滤重复结果
    pub fn deduplicate(&mut self, results: Vec<SearchResult>) -> Vec<SearchResult> {
        let mut unique_results = Vec::new();

        for result in results {
            let is_duplicate = unique_results.iter().any(|existing| {
                self.is_duplicate(&result, existing)
            });

            if !is_duplicate {
                // 添加到已见集合
                self.seen_urls.insert(self.normalize_url(&result.url));
                self.seen_titles.insert(self.normalize_title(&result.title));
                
                if self.config.fingerprint_config.enabled {
                    if let Some(content) = &result.description {
                        let fingerprint = self.generate_fingerprint(content);
                        self.seen_fingerprints.insert(fingerprint);
                    }
                }
                
                unique_results.push(result);
            }
        }

        unique_results
    }

    /// 批量去重多个引擎的结果
    pub fn deduplicate_multi_engine(&mut self, engine_results: Vec<(String, Vec<SearchResult>)>) -> Vec<SearchResult> {
        let mut all_results = Vec::new();
        
        // 合并所有结果
        for (engine_name, results) in engine_results {
            for mut result in results {
                result.source_engine = Some(engine_name.clone());
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
    use crate::domain::entities::search_result::SearchResult;

    fn create_test_result(url: &str, title: &str, description: Option<&str>) -> SearchResult {
        SearchResult {
            title: title.to_string(),
            url: url.to_string(),
            description: description.map(|d| d.to_string()),
            source_engine: None,
            score: 0.0,
            published_date: None,
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
            create_test_result("https://example.com/page3", "Python Programming Guide", None),
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

        let results = vec![
            create_test_result("https://example.com/page1", "Rust Guide", Some("Learn Rust programming")),
            create_test_result("https://example.com/page2", "Rust Programming Guide", Some("Learn Rust programming language")), // 相似标题
            create_test_result("https://example.com/page3", "Python Guide", Some("Learn Python programming")),
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
}