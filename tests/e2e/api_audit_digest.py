#!/usr/bin/env python3
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.
"""从 test-results/api-audit.jsonl 生成人工审查用的 API 交互摘要。

聚焦三类需要人工复核的场景：
  1. 安全脱敏 —— key 签发（防缓存头 + 一次性明文形态）、审计日志脱敏、
     结果敏感头 [REDACTED]、scope 越权 403
  2. 状态转换 —— scrape/crawl 的状态机观测序列、取消 204 → cancelled
  3. 数据过滤 —— team 作用域（auth 覆盖 body）、limit/task_types、
     include_patterns 结果过滤

用法：python3 tests/e2e/api_audit_digest.py \
          [--audit test-results/api-audit.jsonl] \
          [--out test-results/api-interaction-digest.md]
"""

from __future__ import annotations

import argparse
import json
from collections import OrderedDict


def short(body: str, limit: int = 260) -> str:
    body = body or ""
    return body if len(body) <= limit else body[:limit] + "…"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--audit", default="test-results/api-audit.jsonl")
    ap.add_argument("--out", default="test-results/api-interaction-digest.md")
    args = ap.parse_args()

    http: "OrderedDict[str, dict]" = OrderedDict()  # test -> 首次交互
    results: "OrderedDict[str, dict]" = OrderedDict()
    transitions: list[dict] = []
    for line in open(args.audit, encoding="utf-8"):
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("kind") == "http" and row.get("test") not in http:
            http[row["test"]] = row
        elif row.get("kind") == "test_result":
            d = row["detail"]
            if isinstance(d, str):
                d = json.loads(d)
            results[d["name"]] = d
        elif row.get("kind") == "state_transition":
            transitions.append(row)

    # 按审查主题挑选取证点
    focus = {
        "安全脱敏": [
            "apikey_issue", "apikey_readonly_admin", "apikey_new_key_auth",
            "apikey_nil_team", "audit_logs", "scrape_status_poll",
        ],
        "状态转换": [
            "scrape_create", "scrape_status_poll", "crawl_lifecycle_create",
            "crawl_status_poll", "crawl_cancel", "crawl_cancel_status",
        ],
        "数据过滤": [
            "tasks_query_foreign_team", "tasks_query_own_team",
            "tasks_query_limit1", "tasks_query_types",
            "crawl_include_create", "crawl_include_results",
        ],
        "错误包封": [
            "scrape_bad_empty_body", "scrape_bad_malformed_json",
            "ssrf_loopback", "tasks_cancel_empty", "search_empty",
            "teams_me_noauth", "teams_me_badkey",
        ],
    }

    lines = ["# crawlrs API 交互审计摘要（人工审查用）", ""]
    n_pass = sum(1 for d in results.values() if d["status"] == "PASS")
    n_fail = sum(1 for d in results.values() if d["status"] == "FAIL")
    n_skip = sum(1 for d in results.values() if d["status"] == "SKIP")
    lines.append(f"- 用例：{len(results)}（通过 {n_pass} / 失败 {n_fail} / 跳过 {n_skip}）")
    lines.append(f"- 数据来源：`{args.audit}`（原始 JSONL，全部交互逐条记录，凭证已掩码）")
    lines.append("")

    for theme, tests in focus.items():
        lines += [f"## {theme}", ""]
        for t in tests:
            r = http.get(t)
            d = results.get(t)
            if d is None:
                d = next((v for k, v in results.items() if k.startswith(t)), None)
            if not r:
                continue
            status = d["status"] if d else "?"
            lines.append(f"### {t}（用例 {status}）")
            lines.append("```")
            lines.append(f"{r['request']['method']} {r['request']['url']}")
            if r["request"]["body"]:
                lines.append(f"→ 请求体: {short(r['request']['body'])}")
            lines.append(f"← HTTP {r['response']['status']} ({r['duration_ms']} ms)")
            lines.append(f"← 响应体: {short(r['response']['body'])}")
            lines.append("```")
            lines.append("")

    if transitions:
        lines += ["## 状态机观测序列", ""]
        for t in transitions:
            detail = t.get("detail")
            if isinstance(detail, str):
                detail = json.loads(detail)
            lines.append(f"- `{detail.get('task_id') or detail.get('crawl_id')}` "
                         f"观测序列: {detail.get('observed')} → 终态 {detail.get('final')}")
        lines.append("")

    lines += ["## 全量场景结果矩阵", "",
              "| 用例 | 状态 | 失败明细 |", "|---|---|---|"]
    for name, d in results.items():
        fails = "; ".join(d.get("failed_checks", [])) or "—"
        lines.append(f"| {name} | {d['status']} | {fails} |")

    with open(args.out, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    print(f"digest written: {args.out} ({len(results)} scenarios)")


if __name__ == "__main__":
    main()
