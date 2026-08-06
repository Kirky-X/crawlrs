// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in project root for full license information.

//! 知识图谱覆盖感知爬取
//!
//! 在爬取过程中构建知识图谱，追踪已发现的实体和关系，
//! 使用 Chao1 估计器计算覆盖率，并通过结构空洞检测指导
//! URL 优先级排序。

use std::collections::{HashMap, HashSet};

use crate::workers::crawl::ScoringContext;
use crate::workers::crawl::UrlScorer;

/// 实体类型
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum EntityType {
    Person,
    Organization,
    Location,
    Product,
    Event,
    Concept,
    Custom(String),
}

impl EntityType {
    pub fn label(&self) -> &str {
        match self {
            EntityType::Person => "person",
            EntityType::Organization => "organization",
            EntityType::Location => "location",
            EntityType::Product => "product",
            EntityType::Event => "event",
            EntityType::Concept => "concept",
            EntityType::Custom(s) => s.as_str(),
        }
    }
}

/// 关系类型
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum RelationType {
    WorksAt,
    LocatedIn,
    PartOf,
    RelatedTo,
    CreatedBy,
    Custom(String),
}

impl RelationType {
    pub fn label(&self) -> &str {
        match self {
            RelationType::WorksAt => "works_at",
            RelationType::LocatedIn => "located_in",
            RelationType::PartOf => "part_of",
            RelationType::RelatedTo => "related_to",
            RelationType::CreatedBy => "created_by",
            RelationType::Custom(s) => s.as_str(),
        }
    }
}

use serde::{Deserialize, Serialize};

/// 实体节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub entity_type: EntityType,
    pub name: String,
    /// 发现该实体的 URL 集合
    pub source_urls: HashSet<String>,
}

/// 关系边
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub source_id: String,
    pub target_id: String,
    pub relation_type: RelationType,
    /// 发现该关系的 URL 集合
    pub source_urls: HashSet<String>,
}

/// 结构空洞：缺失的实体类型或关系类型
#[derive(Debug, Clone)]
pub struct StructuralHole {
    /// 空洞类型描述
    pub kind: HoleKind,
    /// 相邻实体（与该空洞相关的已发现实体）
    pub adjacent_entities: Vec<String>,
    /// 建议的种子 URL 模式
    pub suggested_url_patterns: Vec<String>,
}

/// 空洞种类
#[derive(Debug, Clone)]
pub enum HoleKind {
    /// 缺失某种实体类型
    MissingEntityType(String),
    /// 两个实体之间缺少某种关系
    MissingRelation {
        entity_a: String,
        entity_b: String,
        relation_type: String,
    },
    /// 某种实体类型数量不足
    UnderrepresentedType {
        entity_type: String,
        current_count: usize,
        expected_min: usize,
    },
}

/// 知识图谱累积器
///
/// 在爬取过程中增量构建，追踪实体节点和关系边。
pub struct KnowledgeGraphAccumulator {
    /// 实体节点：entity_id → Entity
    entities: HashMap<String, Entity>,
    /// 关系边：(source_id, target_id, relation_type) → Relation
    relations: Vec<Relation>,
    /// 实体类型计数：type_label → count
    type_counts: HashMap<String, usize>,
    /// 已知的所有实体类型（包括未发现的）
    known_entity_types: HashSet<String>,
    /// 已知的所有关系类型
    known_relation_types: HashSet<String>,
    /// 已爬取的 URL 集合
    crawled_urls: HashSet<String>,
}

impl KnowledgeGraphAccumulator {
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            relations: Vec::new(),
            type_counts: HashMap::new(),
            known_entity_types: HashSet::new(),
            known_relation_types: HashSet::new(),
            crawled_urls: HashSet::new(),
        }
    }

    /// 注册已知的实体类型（用于结构空洞检测）
    pub fn register_entity_type(&mut self, type_label: &str) {
        self.known_entity_types.insert(type_label.to_string());
    }

    /// 注册已知的关系类型
    pub fn register_relation_type(&mut self, type_label: &str) {
        self.known_relation_types.insert(type_label.to_string());
    }

    /// 添加实体
    pub fn add_entity(
        &mut self,
        id: &str,
        entity_type: EntityType,
        name: &str,
        source_url: &str,
    ) {
        self.crawled_urls.insert(source_url.to_string());

        let entry = self.entities.entry(id.to_string()).or_insert_with(|| Entity {
            id: id.to_string(),
            entity_type: entity_type.clone(),
            name: name.to_string(),
            source_urls: HashSet::new(),
        });
        entry.source_urls.insert(source_url.to_string());

        // 更新类型计数（只在新实体时 +1）
        if entry.source_urls.len() == 1 {
            let count = self.type_counts.entry(entity_type.label().to_string()).or_insert(0);
            *count += 1;
        }
    }

    /// 添加关系
    pub fn add_relation(
        &mut self,
        source_id: &str,
        target_id: &str,
        relation_type: RelationType,
        source_url: &str,
    ) {
        self.crawled_urls.insert(source_url.to_string());

        // 检查是否已存在相同关系
        let exists = self.relations.iter().any(|r| {
            r.source_id == source_id
                && r.target_id == target_id
                && r.relation_type.label() == relation_type.label()
        });

        if exists {
            // 更新 source_urls
            if let Some(r) = self.relations.iter_mut().find(|r| {
                r.source_id == source_id
                    && r.target_id == target_id
                    && r.relation_type.label() == relation_type.label()
            }) {
                r.source_urls.insert(source_url.to_string());
            }
        } else {
            let mut source_urls = HashSet::new();
            source_urls.insert(source_url.to_string());
            self.relations.push(Relation {
                source_id: source_id.to_string(),
                target_id: target_id.to_string(),
                relation_type,
                source_urls,
            });
        }
    }

    /// 发现结构空洞
    ///
    /// 分析当前知识图谱，找出缺失的实体类型、关系类型和数量不足的实体类型。
    pub fn find_structural_holes(&self) -> Vec<StructuralHole> {
        let mut holes = Vec::new();

        // 1. 缺失的实体类型
        for type_label in &self.known_entity_types {
            if !self.type_counts.contains_key(type_label) {
                holes.push(StructuralHole {
                    kind: HoleKind::MissingEntityType(type_label.clone()),
                    adjacent_entities: self.get_all_entity_ids(),
                    suggested_url_patterns: vec![format!("**/{}/**", type_label)],
                });
            }
        }

        // 2. 数量不足的实体类型（少于预期最小值 2）
        let expected_min = 2;
        for (type_label, &count) in &self.type_counts {
            if count < expected_min {
                holes.push(StructuralHole {
                    kind: HoleKind::UnderrepresentedType {
                        entity_type: type_label.clone(),
                        current_count: count,
                        expected_min,
                    },
                    adjacent_entities: self.get_entity_ids_by_type(type_label),
                    suggested_url_patterns: vec![format!("**/{}/**", type_label)],
                });
            }
        }

        // 3. 缺失的关系类型（已知类型中未出现的）
        for rel_type in &self.known_relation_types {
            let has_any = self.relations.iter().any(|r| r.relation_type.label() == rel_type);
            if !has_any {
                // 找可能的实体对
                let entity_ids: Vec<String> = self.entities.keys().cloned().collect();
                if entity_ids.len() >= 2 {
                    holes.push(StructuralHole {
                        kind: HoleKind::MissingRelation {
                            entity_a: entity_ids[0].clone(),
                            entity_b: entity_ids[1].clone(),
                            relation_type: rel_type.clone(),
                        },
                        adjacent_entities: entity_ids,
                        suggested_url_patterns: vec![format!("**/{}/**", rel_type)],
                    });
                }
            }
        }

        holes
    }

    /// Chao1 覆盖率估计
    ///
    /// 基于 singleton（出现 1 次的实体类型）和 doubleton（出现 2 次的实体类型）
    /// 估计总体实体类型丰富度，然后计算已发现类型的覆盖率。
    ///
    /// Chao1 公式：S_est = S_obs + f1^2 / (2 * f2)
    /// 其中 S_obs = 已观察到的类型数，f1 = singleton 数，f2 = doubleton 数
    pub fn estimate_coverage(&self) -> f64 {
        let s_obs = self.type_counts.len() as f64;

        if s_obs == 0.0 {
            return 1.0; // 没有类型时默认全覆盖
        }

        // 计算 singleton 和 doubleton
        let f1 = self.type_counts.values().filter(|&&c| c == 1).count() as f64;
        let f2 = self.type_counts.values().filter(|&&c| c == 2).count() as f64;

        // Chao1 估计
        let s_est = if f2 > 0.0 {
            s_obs + (f1 * f1) / (2.0 * f2)
        } else if f1 > 0.0 {
            // f2 == 0 时的修正公式
            s_obs + (f1 * (f1 - 1.0)) / 2.0
        } else {
            s_obs
        };

        // 覆盖率 = 已发现 / 估计总数
        let coverage = s_obs / s_est;
        coverage.min(1.0)
    }

    /// 获取实体数量
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// 获取关系数量
    pub fn relation_count(&self) -> usize {
        self.relations.len()
    }

    /// 获取已爬取 URL 数量
    pub fn crawled_url_count(&self) -> usize {
        self.crawled_urls.len()
    }

    /// 获取所有实体 ID
    fn get_all_entity_ids(&self) -> Vec<String> {
        self.entities.keys().take(5).cloned().collect()
    }

    /// 按类型获取实体 ID
    fn get_entity_ids_by_type(&self, type_label: &str) -> Vec<String> {
        self.entities
            .values()
            .filter(|e| e.entity_type.label() == type_label)
            .map(|e| e.id.clone())
            .take(5)
            .collect()
    }

    /// 获取指定实体的关系邻居
    pub fn get_neighbors(&self, entity_id: &str) -> Vec<(String, String)> {
        self.relations
            .iter()
            .filter_map(|r| {
                if r.source_id == entity_id {
                    Some((r.target_id.clone(), r.relation_type.label().to_string()))
                } else if r.target_id == entity_id {
                    Some((r.source_id.clone(), r.relation_type.label().to_string()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// 计算 URL 的 KG 优先级提升因子
    ///
    /// 如果 URL 可能填补结构空洞，返回更高的优先级因子。
    pub fn url_priority_boost(&self, url: &str) -> f64 {
        let holes = self.find_structural_holes();
        if holes.is_empty() {
            return 1.0;
        }

        // 检查 URL 是否匹配结构空洞的建议模式
        for hole in &holes {
            for pattern in &hole.suggested_url_patterns {
                if self.url_matches_pattern(url, pattern) {
                    return 2.0; // 匹配空洞模式，优先级翻倍
                }
            }
        }

        1.0
    }

    /// 简单的 URL 模式匹配（支持 `**` 通配符）
    fn url_matches_pattern(&self, url: &str, pattern: &str) -> bool {
        let parts: Vec<&str> = pattern.split("**").collect();
        if parts.len() == 1 {
            return url == pattern;
        }

        let mut pos = 0;
        for part in &parts {
            if part.is_empty() {
                continue;
            }
            if let Some(found) = url[pos..].find(part) {
                pos += found + part.len();
            } else {
                return false;
            }
        }
        true
    }
}

/// KG 优先级提升评分器（T082）
///
/// 实现 `UrlScorer` trait，将 KG 结构空洞检测信号反馈到 URL 优先级。
/// 匹配结构空洞模式的 URL 获得更高分数。
///
/// 用法：添加到 `CompositeScorer` 作为额外评分维度。
pub struct KgBoostScorer {
    kg: std::sync::Arc<std::sync::RwLock<KnowledgeGraphAccumulator>>,
}

impl KgBoostScorer {
    pub fn new(kg: std::sync::Arc<std::sync::RwLock<KnowledgeGraphAccumulator>>) -> Self {
        Self { kg }
    }
}

impl UrlScorer for KgBoostScorer {
    fn score(&self, url: &str, _context: &ScoringContext) -> f32 {
        let kg = self.kg.read().unwrap_or_else(|e| e.into_inner());
        let boost = kg.url_priority_boost(url);
        // boost 范围 [1.0, 2.0]，归一化到 [0.5, 1.0]
        ((boost - 1.0) as f32 + 0.5).clamp(0.0, 1.0)
    }
}

impl Default for KnowledgeGraphAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === T078: KG 构建测试 ===

    #[test]
    fn test_add_entity_and_count() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");
        kg.add_entity("e2", EntityType::Organization, "Acme", "http://example.com/2");
        kg.add_entity("e3", EntityType::Person, "Bob", "http://example.com/3");

        assert_eq!(kg.entity_count(), 3);
        assert_eq!(kg.crawled_url_count(), 3);
    }

    #[test]
    fn test_add_entity_same_id_merges_sources() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/2");

        assert_eq!(kg.entity_count(), 1);
        assert_eq!(kg.crawled_url_count(), 2);
        assert_eq!(kg.entities["e1"].source_urls.len(), 2);
    }

    #[test]
    fn test_add_relation() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");
        kg.add_entity("e2", EntityType::Organization, "Acme", "http://example.com/1");
        kg.add_relation("e1", "e2", RelationType::WorksAt, "http://example.com/1");

        assert_eq!(kg.relation_count(), 1);
    }

    #[test]
    fn test_add_duplicate_relation_merges_sources() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");
        kg.add_entity("e2", EntityType::Organization, "Acme", "http://example.com/1");
        kg.add_relation("e1", "e2", RelationType::WorksAt, "http://example.com/1");
        kg.add_relation("e1", "e2", RelationType::WorksAt, "http://example.com/2");

        assert_eq!(kg.relation_count(), 1);
        assert_eq!(kg.relations[0].source_urls.len(), 2);
    }

    #[test]
    fn test_find_structural_holes_missing_type() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.register_entity_type("person");
        kg.register_entity_type("location");

        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");
        // 没有 location 类型实体

        let holes = kg.find_structural_holes();
        assert!(
            holes.iter().any(|h| matches!(&h.kind, HoleKind::MissingEntityType(t) if t == "location")),
            "should detect missing 'location' type"
        );
    }

    #[test]
    fn test_find_structural_holes_underrepresented() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");
        // 只有 1 个 person，低于 expected_min=2

        let holes = kg.find_structural_holes();
        assert!(
            holes.iter().any(|h| matches!(&h.kind, HoleKind::UnderrepresentedType { entity_type, .. } if entity_type == "person")),
            "should detect underrepresented 'person' type"
        );
    }

    #[test]
    fn test_find_structural_holes_missing_relation() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.register_relation_type("works_at");
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");
        kg.add_entity("e2", EntityType::Organization, "Acme", "http://example.com/2");
        // 没有 works_at 关系

        let holes = kg.find_structural_holes();
        assert!(
            holes.iter().any(|h| matches!(&h.kind, HoleKind::MissingRelation { relation_type, .. } if relation_type == "works_at")),
            "should detect missing 'works_at' relation"
        );
    }

    #[test]
    fn test_get_neighbors() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");
        kg.add_entity("e2", EntityType::Organization, "Acme", "http://example.com/1");
        kg.add_relation("e1", "e2", RelationType::WorksAt, "http://example.com/1");

        let neighbors = kg.get_neighbors("e1");
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].0, "e2");
        assert_eq!(neighbors[0].1, "works_at");
    }

    // === T080: Chao1 覆盖率估计测试 ===

    #[test]
    fn test_estimate_coverage_all_types_well_represented() {
        let mut kg = KnowledgeGraphAccumulator::new();
        // 3 种类型，每种 5 个实体 → 0 singleton, 0 doubleton
        for i in 0..5 {
            kg.add_entity(
                &format!("p{}", i),
                EntityType::Person,
                &format!("Person {}", i),
                &format!("http://example.com/p{}", i),
            );
            kg.add_entity(
                &format!("o{}", i),
                EntityType::Organization,
                &format!("Org {}", i),
                &format!("http://example.com/o{}", i),
            );
            kg.add_entity(
                &format!("l{}", i),
                EntityType::Location,
                &format!("Loc {}", i),
                &format!("http://example.com/l{}", i),
            );
        }

        let coverage = kg.estimate_coverage();
        // 所有类型都有充足代表，覆盖率应为 1.0
        assert!(coverage > 0.9, "coverage should be high when all types well represented, got {}", coverage);
    }

    #[test]
    fn test_estimate_coverage_with_singletons() {
        let mut kg = KnowledgeGraphAccumulator::new();
        // 1 种类型有 10 个实体，2 种类型各有 1 个实体（singleton）
        for i in 0..10 {
            kg.add_entity(
                &format!("p{}", i),
                EntityType::Person,
                &format!("Person {}", i),
                &format!("http://example.com/p{}", i),
            );
        }
        kg.add_entity("o1", EntityType::Organization, "Org1", "http://example.com/o1");
        kg.add_entity("l1", EntityType::Location, "Loc1", "http://example.com/l1");

        let coverage = kg.estimate_coverage();
        // 3 种类型已发现，但有 singleton → Chao1 估计 > 3 → 覆盖率 < 1.0
        assert!(coverage < 1.0, "coverage should be < 1.0 with singletons, got {}", coverage);
        assert!(coverage > 0.5, "coverage should still be reasonable, got {}", coverage);
    }

    #[test]
    fn test_estimate_coverage_empty() {
        let kg = KnowledgeGraphAccumulator::new();
        let coverage = kg.estimate_coverage();
        assert_eq!(coverage, 1.0, "empty KG should have default coverage 1.0");
    }

    #[test]
    fn test_estimate_coverage_single_type() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");

        let coverage = kg.estimate_coverage();
        // 1 种类型，1 个实体（singleton），Chao1 估计 S_est = 1 + 0 = 1
        // coverage = 1/1 = 1.0
        assert!(coverage >= 0.5, "single type coverage should be reasonable, got {}", coverage);
    }

    // === URL 优先级提升测试 ===

    #[test]
    fn test_url_priority_boost_no_holes() {
        let mut kg = KnowledgeGraphAccumulator::new();
        // 添加足够实体，无结构空洞
        for i in 0..3 {
            kg.add_entity(
                &format!("p{}", i),
                EntityType::Person,
                &format!("Person {}", i),
                &format!("http://example.com/p{}", i),
            );
        }

        let boost = kg.url_priority_boost("http://example.com/new");
        // 如果无空洞，boost = 1.0
        assert!(boost >= 1.0);
    }

    #[test]
    fn test_url_priority_boost_with_matching_hole() {
        let mut kg = KnowledgeGraphAccumulator::new();
        kg.register_entity_type("location");
        // 只有 person，没有 location → 结构空洞
        kg.add_entity("e1", EntityType::Person, "Alice", "http://example.com/1");
        kg.add_entity("e2", EntityType::Person, "Bob", "http://example.com/2");

        let boost = kg.url_priority_boost("http://example.com/location/paris");
        assert_eq!(boost, 2.0, "URL matching hole pattern should get 2x boost");
    }
}
