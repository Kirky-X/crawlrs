// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 混合 DOM+AX 树快照（F12）
//!
//! 经 CDP `Accessibility.getFullAXTree` 取页面无障碍树，裁剪（prune）与
//! 序列化（serialize）为 `[backendNodeId] role: name` 文本大纲，作为与截图
//! 并存的第二通道喂给视觉模型（差量增强，非替换）。
//!
//! - **prune**：`ignored == true` 子树整体丢弃（aria-hidden 语义）；无名且
//!   子数 ≤ 1 的结构节点（Generic/None/InlineTextBox 类）丢弃或单子提升
//!   （对齐 Stagehand 多层裁剪）；无名多子节点仅作连接器保留。
//! - **serialize**：DFS 生成行，单子链因 prune 的提升自然折叠为一行；
//!   `[id]` 为 backendNodeId——Click 决策的 `ax_id` 直取该值经
//!   `DOM.resolveNode` 定位，省 selector 猜测。
//! - **预算**：输出总字符受 `max_token_budget` 约束，按行截断并末行标注
//!   truncated（截断偏头部，SPA 大页建议配合目标元素位置选用，见蓝图风险）。

use crate::engines::engine_client::EngineError;
use chromiumoxide::cdp::browser_protocol::accessibility::{AxNode, GetFullAxTreeParams};
use std::collections::HashMap;

/// 经 CDP 取全量 AX 树（root document；SPA 大页节点数可达数千，调用方必须
/// 走 prune + 预算截断护栏）
pub async fn capture(page: &chromiumoxide::Page) -> Result<Vec<AxNode>, EngineError> {
    let response = page
        .execute(GetFullAxTreeParams::default())
        .await
        .map_err(|e| EngineError::BrowserError(format!("GetFullAXTree failed: {}", e)))?;
    Ok(response.result.nodes)
}

/// 裁剪 AX 树（flat 入 / flat 出）：DFS 顺序输出保留节点，`parent_id` 与
/// `child_ids` 重写为裁剪后的邻接关系（无名结构节点提升后父子直连）
pub fn prune(nodes: Vec<AxNode>) -> Vec<AxNode> {
    let (children, roots) = children_index(&nodes);
    let kept: Vec<KeptNode> = roots
        .into_iter()
        .flat_map(|r| prune_node(&nodes, &children, r))
        .collect();
    let mut out = Vec::new();
    flatten_kept(&nodes, &kept, None, &mut out);
    out
}

/// 序列化为 `[backendNodeId] role: name` 行大纲；总字符 ≤ `budget_chars`，
/// 超出按行截断并末行标注 truncated。仅输出「有名 + 可定位（含
/// backendNodeId）」节点的行，无名节点只作连接器递归子级。
pub fn serialize(nodes: &[AxNode], budget_chars: usize) -> String {
    let (children, roots) = children_index(nodes);
    let mut out = String::new();
    let mut truncated = false;
    for &root in &roots {
        let mut stack = vec![root];
        while let Some(idx) = stack.pop() {
            let node = &nodes[idx];
            for &child in children[idx].iter().rev() {
                stack.push(child);
            }
            if let Some(line) = outline_line(node) {
                let add = out.len() + if out.is_empty() { 0 } else { 1 } + line.len();
                if add > budget_chars {
                    truncated = true;
                    stack.clear();
                    break;
                }
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&line);
            }
        }
        if truncated {
            break;
        }
    }
    if truncated {
        // 标注放不下时回退丢行腾位；丢到只剩空仍放不下（budget < 标注长）则不标注
        while !out.is_empty() && out.len() + 1 + TRUNCATED_MARKER.len() > budget_chars {
            match out.rfind('\n') {
                Some(p) => out.truncate(p),
                None => out.clear(),
            }
        }
        let marker_add = if out.is_empty() { 0 } else { 1 } + TRUNCATED_MARKER.len();
        if out.len() + marker_add <= budget_chars {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(TRUNCATED_MARKER);
        }
    }
    out
}

/// 快照大纲一步式入口：prune + serialize
pub fn outline(nodes: Vec<AxNode>, budget_chars: usize) -> String {
    serialize(&prune(nodes), budget_chars)
}

/// 末行截断标注（蓝图口径：标注 truncated）
const TRUNCATED_MARKER: &str = "...truncated";

/// 命名判定：`name.value` 为非空白字符串
fn has_name(node: &AxNode) -> bool {
    node.name
        .as_ref()
        .and_then(|v| v.value.as_ref())
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty())
}

/// 单行输出：`[{backendNodeId}] {role 小写}: {name}`；无名或缺
/// backendNodeId（无法被 `ax_id` 引用定位）的节点不出行
fn outline_line(node: &AxNode) -> Option<String> {
    let name = node
        .name
        .as_ref()?
        .value
        .as_ref()?
        .as_str()?
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let backend_id = node.backend_dom_node_id?;
    let role = node
        .role
        .as_ref()
        .and_then(|r| r.value.as_ref())
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_lowercase();
    Some(format!("[{}] {}: {}", backend_id.inner(), role, name))
}

/// 按父指针建子邻接表：返回 `(children, roots)`；父缺失的节点视为根
fn children_index(nodes: &[AxNode]) -> (Vec<Vec<usize>>, Vec<usize>) {
    let id_to_idx: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.node_id.inner().as_str(), i))
        .collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut is_child = vec![false; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if let Some(pid) = &n.parent_id {
            if let Some(&pi) = id_to_idx.get(pid.inner().as_str()) {
                children[pi].push(i);
                is_child[i] = true;
            }
        }
    }
    let roots: Vec<usize> = (0..nodes.len()).filter(|&i| !is_child[i]).collect();
    (children, roots)
}

/// 递归裁剪：返回该子树应挂到父级的保留节点（无名且保留子 ≤ 1 → 提升/丢弃）
fn prune_node(nodes: &[AxNode], children: &[Vec<usize>], idx: usize) -> Vec<KeptNode> {
    if nodes[idx].ignored {
        return Vec::new();
    }
    let kept_children: Vec<KeptNode> = children[idx]
        .iter()
        .flat_map(|&c| prune_node(nodes, children, c))
        .collect();
    if !has_name(&nodes[idx]) && kept_children.len() <= 1 {
        return kept_children;
    }
    vec![KeptNode {
        idx,
        children: kept_children,
    }]
}

/// 裁剪后的树节点（下标 + 保留子树）
struct KeptNode {
    idx: usize,
    children: Vec<KeptNode>,
}

/// DFS 摊平保留树：重写 `parent_id` / `child_ids` 为裁剪后邻接关系
fn flatten_kept(
    nodes: &[AxNode],
    kept: &[KeptNode],
    parent_idx: Option<usize>,
    out: &mut Vec<AxNode>,
) {
    for k in kept {
        let mut node = nodes[k.idx].clone();
        node.parent_id = parent_idx.map(|p| nodes[p].node_id.clone());
        let child_ids: Vec<chromiumoxide::cdp::browser_protocol::accessibility::AxNodeId> = k
            .children
            .iter()
            .map(|c| nodes[c.idx].node_id.clone())
            .collect();
        node.child_ids = if child_ids.is_empty() {
            None
        } else {
            Some(child_ids)
        };
        let my_idx = k.idx;
        out.push(node);
        flatten_kept(nodes, &k.children, Some(my_idx), out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chromiumoxide::cdp::browser_protocol::accessibility::{AxNodeId, AxValue, AxValueType};
    use chromiumoxide::cdp::browser_protocol::dom::BackendNodeId;

    /// 构造测试节点：`ax_node("1", Some("1"), "heading", "Title", 11, false)`
    fn ax_node(
        id: &str,
        parent: Option<&str>,
        role: Option<&str>,
        name: Option<&str>,
        backend: Option<i64>,
        ignored: bool,
    ) -> AxNode {
        let mut n = AxNode::new(AxNodeId::new(id), ignored);
        n.parent_id = parent.map(AxNodeId::new);
        n.role = role.map(|r| AxValue {
            r#type: AxValueType::Role,
            value: Some(serde_json::Value::String(r.to_string())),
            related_nodes: None,
            sources: None,
        });
        n.name = name.map(|nm| AxValue {
            r#type: AxValueType::String,
            value: Some(serde_json::Value::String(nm.to_string())),
            related_nodes: None,
            sources: None,
        });
        n.backend_dom_node_id = backend.map(BackendNodeId::new);
        n
    }

    // ===== prune =====

    /// ignored 子树整体丢弃（含其后代）
    #[test]
    fn prune_drops_ignored_subtree() {
        let nodes = vec![
            ax_node("1", None, Some("generic"), Some("root"), Some(1), false),
            ax_node("2", Some("1"), Some("banner"), None, Some(2), true),
            ax_node(
                "3",
                Some("2"),
                Some("heading"),
                Some("hidden heading"),
                Some(3),
                false,
            ),
            ax_node(
                "4",
                Some("1"),
                Some("main"),
                Some("content"),
                Some(4),
                false,
            ),
        ];
        let pruned = prune(nodes);
        let ids: Vec<&str> = pruned.iter().map(|n| n.node_id.inner().as_str()).collect();
        assert_eq!(ids, vec!["1", "4"], "ignored 子树（2 及其后代 3）整体丢弃");
    }

    /// 无名叶子（Generic 类）被丢；无名单子节点提升
    #[test]
    fn prune_drops_unnamed_leaves_and_promotes_single_child() {
        let nodes = vec![
            ax_node("1", None, Some("generic"), None, Some(1), false),
            ax_node("2", Some("1"), Some("generic"), None, Some(2), false),
            ax_node("3", Some("2"), Some("generic"), None, Some(3), false),
            ax_node(
                "4",
                Some("3"),
                Some("link"),
                Some("Sign in"),
                Some(4),
                false,
            ),
            ax_node("5", Some("1"), Some("generic"), None, Some(5), false),
        ];
        let pruned = prune(nodes);
        let ids: Vec<&str> = pruned.iter().map(|n| n.node_id.inner().as_str()).collect();
        assert_eq!(
            ids,
            vec!["4"],
            "无名链 1>2>3 折叠提升，仅留有名叶子 4；无名叶子 5 丢弃"
        );
        let top = &pruned[0];
        assert_eq!(top.parent_id, None, "提升后根节点无父");
    }

    /// 无名多子节点作连接器保留（子级不丢）
    #[test]
    fn prune_keeps_unnamed_connector_with_multiple_children() {
        let nodes = vec![
            ax_node("1", None, Some("generic"), None, Some(1), false),
            ax_node("2", Some("1"), Some("link"), Some("A"), Some(2), false),
            ax_node("3", Some("1"), Some("link"), Some("B"), Some(3), false),
        ];
        let pruned = prune(nodes);
        assert_eq!(pruned.len(), 3, "无名双子容器保留为连接器");
    }

    // ===== serialize =====

    /// 每行匹配 `^\[\d+\] [a-z]+: .+$`（role 小写）
    #[test]
    fn serialize_line_format_matches_protocol() {
        let nodes = vec![
            ax_node("1", None, Some("Main"), Some("Page"), Some(11), false),
            ax_node(
                "2",
                Some("1"),
                Some("heading"),
                Some("  Welcome  "),
                Some(12),
                false,
            ),
            ax_node(
                "3",
                Some("1"),
                Some("button"),
                Some("Submit"),
                Some(13),
                false,
            ),
        ];
        let out = serialize(&nodes, 4096);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert!(line.starts_with('['), "行须以 [backendNodeId] 开头: {line}");
            let rest = line.split_once("] ").expect("] 分隔").1;
            let (role, name) = rest.split_once(": ").expect(": 分隔");
            assert!(
                !role.is_empty() && role.chars().all(|c| c.is_ascii_lowercase()),
                "role 须全小写: {role}"
            );
            assert!(!name.is_empty(), "行须带非空 name: {line}");
        }
        assert!(lines.contains(&"[12] heading: Welcome"), "name 须 trim");
        assert!(lines.contains(&"[11] main: Page"), "role 须小写化");
    }

    /// 无名连接器与缺 backendNodeId 的节点不出行，但子级照常递归
    #[test]
    fn serialize_skips_connector_and_unresolvable_nodes() {
        let nodes = vec![
            ax_node("1", None, Some("generic"), None, Some(1), false),
            ax_node("2", Some("1"), Some("navigation"), Some("Nav"), None, false),
            ax_node("3", Some("2"), Some("link"), Some("Home"), Some(3), false),
        ];
        let out = serialize(&nodes, 4096);
        assert_eq!(out, "[3] link: Home", "仅可定位的有名节点出行");
    }

    /// 单子链折叠为一行（div>div>span 场景）
    #[test]
    fn serialize_folds_single_child_chain_to_one_line() {
        let nodes = vec![
            ax_node("1", None, Some("generic"), None, Some(1), false),
            ax_node("2", Some("1"), Some("generic"), None, Some(2), false),
            ax_node(
                "3",
                Some("2"),
                Some("text"),
                Some("hello world"),
                Some(3),
                false,
            ),
        ];
        let out = outline(nodes, 4096);
        assert_eq!(out, "[3] text: hello world");
    }

    /// 预算截断：千节点树超预算 → 输出含 truncated 标记且长度 ≤ budget
    #[test]
    fn serialize_truncates_at_budget_with_marker() {
        let mut nodes = Vec::new();
        for i in 0..1000 {
            let parent = if i <= 1 { None } else { Some("1") };
            nodes.push(ax_node(
                &i.to_string(),
                parent,
                Some("option"),
                Some(&format!("option item number {i}")),
                Some(i),
                false,
            ));
        }
        let budget = 500usize;
        let out = outline(nodes, budget);
        assert!(out.contains("truncated"), "须含截断标注");
        assert!(out.len() <= budget, "len {} > budget {}", out.len(), budget);
        assert!(out.lines().count() >= 5, "预算内应已装下多行: {out}");
        assert!(out.lines().last().unwrap() == "...truncated");
    }

    /// 预算充足时完整输出、无 truncated 标记
    #[test]
    fn serialize_within_budget_has_no_marker() {
        let nodes = vec![ax_node(
            "1",
            None,
            Some("heading"),
            Some("Title"),
            Some(1),
            false,
        )];
        let out = serialize(&nodes, 4096);
        assert_eq!(out, "[1] heading: Title");
        assert!(!out.contains("truncated"));
    }

    /// 空树/全无名树输出空串（不 panic）
    #[test]
    fn serialize_empty_and_unnamed_trees() {
        assert_eq!(serialize(&[], 100), "");
        let unnamed = vec![ax_node("1", None, Some("generic"), None, Some(1), false)];
        assert_eq!(serialize(&unnamed, 100), "");
    }

    /// ignored=false 且 role 缺失的命名节点回退 unknown role
    #[test]
    fn serialize_missing_role_falls_back_to_unknown() {
        let mut n = ax_node("1", None, None, Some("Odd"), Some(1), false);
        n.role = None;
        assert_eq!(serialize(&[n], 100), "[1] unknown: Odd");
    }
}
