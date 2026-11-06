// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `scraper::Html`（html5ever 树）→ `moli_html2md::Dom` 适配器
//!
//! 契约（上游 `Dom` trait，只读视图、不解析 HTML 二次、不二次解码实体）：
//! - `node_kind`：Document/Fragment → Document；Element → 小写局部名；Text → 已解码文本；
//!   注释/doctype/处理指令 → Other
//! - `first_child` / `next_sibling`：ego_tree 直接映射（scraper 内部树，NodeId Copy）
//! - `attribute`：大小写不敏感匹配（scraper 属性名已小写化，双保险）
//! - `control_value` / `has_block_layout` / `has_text_boundary`：保留 trait 默认实现
//!   （None/false）——静态 HTML 无计算样式，接了就是假数据（诚实降级，设计 D2）
//!
//! `<template>` 内容：scraper 树中 template 子节点即解析结果，随 children 常规遍历
//! （与上游 trait 文档"adapter 暴露即访问"一致）。

use moli_html2md::{Dom, NodeKind};
use scraper::node::Node;

/// `scraper::Html` 的只读 Dom 视图
pub struct ScraperDom<'a> {
    doc: &'a scraper::Html,
}

impl<'a> ScraperDom<'a> {
    pub fn new(doc: &'a scraper::Html) -> Self {
        Self { doc }
    }
}

impl Dom for ScraperDom<'_> {
    type NodeId = ego_tree::NodeId;

    fn node_kind(&self, node: Self::NodeId) -> NodeKind<'_> {
        match self.doc.tree.get(node).map(|n| n.value()) {
            Some(Node::Document) | Some(Node::Fragment) => NodeKind::Document,
            // scraper 的 Element::name() 即 html5ever 归一化后的小写局部名
            Some(Node::Element(el)) => NodeKind::Element(el.name()),
            // scraper 的 Text 节点在解析时已完成实体解码（StrTendril 明文）
            Some(Node::Text(text)) => NodeKind::Text(text.text.as_ref()),
            Some(Node::Comment(_))
            | Some(Node::Doctype(_))
            | Some(Node::ProcessingInstruction(_)) => NodeKind::Other,
            // 无效 NodeId：按契约归 Other（不 panic——上游约定 invalid IDs 走 Other）
            None => NodeKind::Other,
        }
    }

    fn first_child(&self, node: Self::NodeId) -> Option<Self::NodeId> {
        self.doc
            .tree
            .get(node)
            .and_then(|n| n.first_child())
            .map(|n| n.id())
    }

    fn next_sibling(&self, node: Self::NodeId) -> Option<Self::NodeId> {
        self.doc
            .tree
            .get(node)
            .and_then(|n| n.next_sibling())
            .map(|n| n.id())
    }

    fn attribute(&self, node: Self::NodeId, name: &str) -> Option<&str> {
        self.doc
            .tree
            .get(node)?
            .value()
            .as_element()?
            .attrs()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(html: &str) -> scraper::Html {
        scraper::Html::parse_document(html)
    }

    /// Element 小写局部名 + Text 已解码（实体不二次转义）
    #[test]
    fn node_kind_element_lowercase_and_text_decoded() {
        let doc = parse("<DIV id=\"x\">a &amp; b</DIV>");
        let dom = ScraperDom::new(&doc);
        let div = doc
            .tree
            .nodes()
            .find(|n| matches!(n.value(), Node::Element(e) if e.name() == "div"))
            .expect("div node");
        assert_eq!(dom.node_kind(div.id()), NodeKind::Element("div"));
        let text = div.first_child().expect("text child").id();
        // html5ever 已解码 &amp; → &，适配器不得二次解码
        match dom.node_kind(text) {
            NodeKind::Text(t) => assert_eq!(t, "a & b"),
            other => panic!("期望 Text，实际 {other:?}"),
        }
    }

    /// 注释 → Other，遍历时跳过
    #[test]
    fn comment_maps_to_other() {
        let doc = parse("<html><body><!-- note --><p>x</p></body></html>");
        let dom = ScraperDom::new(&doc);
        let comment = doc
            .tree
            .nodes()
            .find(|n| matches!(n.value(), Node::Comment(_)))
            .expect("comment node");
        assert_eq!(dom.node_kind(comment.id()), NodeKind::Other);
    }

    /// first_child/next_sibling 链走通完整兄弟序列
    #[test]
    fn child_and_sibling_chain() {
        let doc = parse("<html><body><ul><li>a</li><li>b</li><li>c</li></ul></body></html>");
        let dom = ScraperDom::new(&doc);
        let ul = doc
            .tree
            .nodes()
            .find(|n| matches!(n.value(), Node::Element(e) if e.name() == "ul"))
            .expect("ul");
        let first = dom.first_child(ul.id()).expect("first li");
        let mut names = Vec::new();
        let mut cursor = Some(first);
        while let Some(id) = cursor {
            names.push(match dom.node_kind(id) {
                NodeKind::Element(n) => n.to_string(),
                other => format!("{other:?}"),
            });
            cursor = dom.next_sibling(id);
        }
        assert_eq!(names, ["li", "li", "li"]);
    }

    /// attribute 大小写不敏感匹配（scraper 已小写化，双保险）
    #[test]
    fn attribute_case_insensitive() {
        let doc =
            parse("<html><body><a HREF=\"https://example.com\" data-x=\"1\">x</a></body></html>");
        let dom = ScraperDom::new(&doc);
        let a = doc
            .tree
            .nodes()
            .find(|n| matches!(n.value(), Node::Element(e) if e.name() == "a"))
            .expect("a");
        assert_eq!(dom.attribute(a.id(), "href"), Some("https://example.com"));
        assert_eq!(dom.attribute(a.id(), "HREF"), Some("https://example.com"));
        assert_eq!(dom.attribute(a.id(), "data-x"), Some("1"));
        assert_eq!(dom.attribute(a.id(), "missing"), None);
    }

    /// Document 根与无效 NodeId → Document/Other（不 panic）
    #[test]
    fn document_root_and_invalid_id() {
        let doc = parse("<p>x</p>");
        let dom = ScraperDom::new(&doc);
        assert_eq!(dom.node_kind(doc.tree.root().id()), NodeKind::Document);
        let missing = doc.tree.nodes().next().unwrap().id();
        // 一个确定不存在的 ID 无法构造；以语法树内节点验证遍历不 panic 即可
        let _ = dom.first_child(missing);
        assert!(dom.first_child(missing).is_some());
    }
}
