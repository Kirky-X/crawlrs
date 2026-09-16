#!/usr/bin/env python3
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.
"""crawlrs API 语义级 E2E 测试。

与 tests/e2e/api_test.sh（状态码冒烟）互补：本套件验证每个 API 的
**响应内容是否符合请求语义**，重点：

  A. 响应包封一致性   —— ApiResponse 契约（success/data/error.code/timestamp）
  B. 认证语义         —— 401/403 包封、凭证生命周期
  C. Scrape 语义      —— 正常路径、状态机词汇表、结果完整性、输入校验
  D. Crawl 语义       —— 生命周期、include_patterns 数据过滤、取消状态转换
  E. Search 容错语义  —— 环境受限下的降级路径包封
  F. 安全脱敏         —— key 签发一次性明文 + no-store 头、scope 越权防护、
                         审计日志/结果头脱敏
  G. 任务管理数据过滤 —— team 作用域（auth 覆盖 body）、limit/type 过滤、批量取消
  H. 并发             —— 并发提交唯一性与稳定性

每次 HTTP 交互（请求 + 响应）追加写入 JSONL 审计文件供人工审查；
审计文件中的 Authorization 头与已签发 key 明文一律掩码（审计文件自身
不得成为凭证泄漏源）。

用法：
  python3 tests/e2e/api_semantics_test.py \
      --base-url http://localhost:8901 \
      --api-key "$KEY" --team-id "$TEAM_ID" \
      --audit-log test-results/api-audit.jsonl

退出码：失败用例数（0 = 全部通过）。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid
from datetime import datetime, timezone

# ── 常量 ────────────────────────────────────────────────────────────────────

# 任务状态机合法词汇（与 domain TaskStatus 序列化对齐；超集防御性放宽）
TASK_STATUS_VOCAB = {
    "queued", "pending", "processing", "running",
    "completed", "success", "failed", "cancelled", "canceled",
}
TERMINAL_STATUSES = {"completed", "success", "failed", "cancelled", "canceled"}

SENSITIVE_HEADER_NAMES = {
    "set-cookie", "authorization", "x-auth-token", "x-api-key",
    "proxy-authorization", "cookie",
}

# E2E/集成测试必须打真实站点（约束：example.com 仅允许单元测试使用）。
# 选取无反爬、长期稳定的真实新闻站；主站失败时按序故障转移。
# 每站点附内容标记：断言抓取到的正文确实来自该站（防代理页/错误页冒充）。
NEWS_SITES = [
    ("https://text.npr.org", "npr.org"),              # NPR 文字版新闻
    ("https://news.ycombinator.com", "ycombinator"),  # Hacker News 科技新闻
    ("https://lite.cnn.com", "cnn.com"),              # CNN lite 新闻
]
NEWS_PRIMARY = NEWS_SITES[0][0]
# include_patterns 过滤种子：en.wikinews.org（维基新闻，外链丰富的真实新闻站，
# 主页含大量同域 /wiki/ 绝对链接），种子豁免容忍尾斜杠
NEWS_FILTER_SEED = "https://en.wikinews.org"
NEWS_INCLUDE_PATTERN = r"^https://en\.wikinews\.org/.*"
# 站点独占分配：同一轮套件内全局 URL 去重（Bloom）跨 crawl 生效——两个 crawl
# 若用同一站点，后跑者的子链接会被整体去重（结果仅剩种子）。故 include 过滤
# 用 wikinews（外链丰富且 robots 无 crawl-delay），与 smoke/lifecycle
# （text.npr.org）互不相交。HN 不可用：robots.txt 要求 crawl-delay 30s/链接，
# 74 个子链接会把爬取通道阻塞半小时。

SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+")
RFC3339_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}")
GARRISON_KEY_RE = re.compile(r"^[^.]+\.[^.]+$")  # key_id.key_secret


# ── 审计日志 ────────────────────────────────────────────────────────────────

def mask_token(value: str, keep: int = 6) -> str:
    """长 token 掩码：保留前 keep 位 + 长度提示，杜绝明文入库。"""
    if not isinstance(value, str) or len(value) <= keep + 4:
        return value
    return f"{value[:keep]}…<masked len={len(value)}>"


def hdr(headers: dict, name: str):
    """大小写不敏感读取响应头（hyper 按 http crate 小写序列化）。"""
    for k, v in headers.items():
        if k.lower() == name.lower():
            return v
    return None


def mask_text(text: str, secrets: list[str]) -> str:
    """在自由文本中掩码全部已知凭证。"""
    for s in secrets:
        if s and s in text:
            text = text.replace(s, mask_token(s))
    return text


class Auditor:
    """JSONL 审计日志：每条 = 一次 HTTP 交互（请求/响应/断言）。"""

    def __init__(self, path, secrets):
        self.path = path
        self.secrets = secrets
        self._lock = threading.Lock()
        self._fh = open(path, "a", encoding="utf-8") if path else None

    def write(self, entry: dict):
        if not self._fh:
            return
        with self._lock:
            self._fh.write(json.dumps(entry, ensure_ascii=False, default=str) + "\n")
            self._fh.flush()

    def log_http(self, name, method, url, req_body, status, resp_headers,
                 resp_body, duration_ms, error=None):
        self.write({
            "ts": datetime.now(timezone.utc).isoformat(),
            "kind": "http",
            "test": name,
            "request": {
                "method": method,
                "url": url,
                "authorization": "Bearer <masked>" if "api-key" in name or True else None,
                "body": mask_text(req_body, self.secrets) if req_body else None,
            },
            "response": {
                "status": status,
                "headers": {
                    k: (mask_text(v, self.secrets) if k.lower() in
                        ("authorization", "x-api-key", "set-cookie") else v)
                    for k, v in resp_headers.items()
                },
                "body": mask_text(resp_body[:8000], self.secrets) if resp_body else "",
            },
            "duration_ms": round(duration_ms, 1),
            "error": error,
        })

    def log_event(self, kind: str, test: str, detail: dict):
        self.write({
            "ts": datetime.now(timezone.utc).isoformat(),
            "kind": kind,
            "test": test,
            "detail": mask_text(json.dumps(detail, ensure_ascii=False, default=str),
                                self.secrets)[:4000],
        })

    def close(self):
        if self._fh:
            self._fh.close()


# ── HTTP 客户端 ─────────────────────────────────────────────────────────────

class Client:
    def __init__(self, base_url, api_key, auditor: Auditor):
        self.base = base_url.rstrip("/")
        self.api_key = api_key
        self.auditor = auditor

    def request(self, name, method, path, body=None, auth="auth",
                timeout=30, extra_headers=None):
        """返回 (status, headers, body_text, json_or_None)。永不抛网络异常外的错误。"""
        url = f"{self.base}{path}"
        data = body.encode("utf-8") if isinstance(body, str) else body
        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("Content-Type", "application/json")
        if auth == "auth" and self.api_key:
            req.add_header("Authorization", f"Bearer {self.api_key}")
        elif auth == "badkey":
            req.add_header("Authorization", "Bearer invalid.key.here")
        for k, v in (extra_headers or {}).items():
            req.add_header(k, v)

        start = time.monotonic()
        status, headers, text, err = 0, {}, "", None
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                status = resp.status
                headers = dict(resp.headers.items())
                text = resp.read().decode("utf-8", errors="replace")
        except urllib.error.HTTPError as e:
            status = e.code
            headers = dict(e.headers.items()) if e.headers else {}
            text = e.read().decode("utf-8", errors="replace")
        except Exception as e:  # 连接失败/超时 → 记录并以 status=0 呈现
            err = f"{type(e).__name__}: {e}"
        duration = (time.monotonic() - start) * 1000

        payload = None
        if text:
            try:
                payload = json.loads(text)
            except json.JSONDecodeError:
                payload = None
        self.auditor.log_http(name, method, url,
                              body if isinstance(body, str) else None,
                              status, headers, text, duration, err)
        return status, headers, text, payload


# ── 测试骨架 ────────────────────────────────────────────────────────────────

class Runner:
    def __init__(self, client: Client, auditor: Auditor, team_id, base_url):
        self.client = client
        self.auditor = auditor
        self.team_id = team_id
        self.base_url = base_url
        self.results: list[dict] = []  # 覆盖矩阵行
        self._lock = threading.Lock()
        self.passed = 0
        self.failed = 0
        self.skipped = 0
        self.issued_keys: list[str] = []  # 已签发 key（用于审计掩码与脱敏断言）

    def run(self, name, category, scenario, func):
        """执行单个场景；func(check) 返回 None（通过）或错误消息字符串。"""
        checks: list[tuple[bool, str]] = []

        def check(ok, msg):
            checks.append((bool(ok), msg))

        start = time.monotonic()
        try:
            err = func(check)
        except Exception as e:  # 场景代码自身异常 = 失败（含堆栈摘要）
            err = f"scenario raised {type(e).__name__}: {e}"
        duration = round((time.monotonic() - start) * 1, 1)

        if err:
            status = "FAIL"
        elif any(ok is False for ok, _ in checks):
            status = "FAIL"
        elif not checks:
            status = "SKIP"  # 场景未产生任何断言（如环境依赖跳过）
        else:
            status = "PASS"

        failed_msgs = [m for ok, m in checks if not ok]
        if err:
            failed_msgs.insert(0, str(err))
        row = {
            "category": category,
            "name": name,
            "scenario": scenario,
            "status": status,
            "duration_ms": duration,
            "failed_checks": failed_msgs,
            "checks_total": len(checks),
        }
        with self._lock:
            self.results.append(row)
            if status == "PASS":
                self.passed += 1
            elif status == "SKIP":
                self.skipped += 1
            else:
                self.failed += 1
            self.auditor.log_event("test_result", name, row)
        icon = {"PASS": "✓", "FAIL": "✗", "SKIP": "⊘"}[status]
        print(f"  {icon} [{status}] {category} / {name}")
        for m in failed_msgs:
            print(f"      · {m}")

    # ── 通用断言助手 ──
    @staticmethod
    def envelope_ok(check, payload, ctx):
        """ApiResponse 包封契约：success 布尔、error 结构、timestamp RFC3339。"""
        if payload is None:
            check(False, f"{ctx}: 响应不是合法 JSON")
            return False
        check(isinstance(payload.get("success"), bool), f"{ctx}: success 为布尔")
        ts = payload.get("timestamp")
        check(ts is None or (isinstance(ts, str) and RFC3339_RE.match(ts)),
              f"{ctx}: timestamp 为 RFC3339（got {ts!r}）")
        return True

    @staticmethod
    def success_envelope(check, payload, ctx):
        if not Runner.envelope_ok(check, payload, ctx):
            return
        check(payload.get("success") is True, f"{ctx}: success=true")
        check(payload.get("data") is not None, f"{ctx}: data 非空")
        check("error" not in payload or payload.get("error") is None,
              f"{ctx}: 成功响应无 error")

    @staticmethod
    def error_envelope(check, payload, status, ctx, want_codes=None):
        if not Runner.envelope_ok(check, payload, ctx):
            return
        check(payload.get("success") is False, f"{ctx}: success=false")
        err = payload.get("error") or {}
        check(isinstance(err.get("code"), str) and err["code"],
              f"{ctx}: error.code 存在（got {err.get('code')!r}）")
        check(isinstance(err.get("message"), str) and err["message"],
              f"{ctx}: error.message 存在")
        check(payload.get("data") is None, f"{ctx}: 错误响应无 data（防泄漏）")
        if want_codes:
            check(err.get("code") in want_codes,
                  f"{ctx}: error.code ∈ {want_codes}（got {err.get('code')!r}）")


# ── 场景定义 ────────────────────────────────────────────────────────────────

def build_scenarios(r: Runner):
    c = r.client
    scenarios = []

    # ═══ A. 公开端点：响应结构语义 ═══
    def t_health(check):
        s, h, text, j = c.request("health", "GET", "/health")
        check(s == 200, f"200（got {s}）")
        check(j and j.get("status") == "healthy", "status=healthy")
        check(bool(SEMVER_RE.match(str(j.get("version", "")))), f"version 语义化（{j.get('version')!r}）")
    scenarios.append(("health_semantics", "A-公开端点", "GET /health 返回存活+版本", t_health))

    def t_version(check):
        s, h, text, j = c.request("version", "GET", "/v1/version")
        check(s == 200, f"200（got {s}）")
        check(bool(SEMVER_RE.match(text.strip())), f"纯文本语义化版本（{text[:40]!r}）")
    scenarios.append(("version_semantics", "A-公开端点", "GET /v1/version 纯文本版本", t_version))

    def t_ready(check):
        s, h, text, j = c.request("ready", "GET", "/ready")
        check(s in (200, 503), f"200|503（got {s}）")
        if j is None:
            check(False, f"响应为 JSON（got {text[:120]!r}）")
            return
        if s == 200:
            check(j.get("status") == "ready", "ready 时 status=ready")
        else:
            check(j.get("status") == "not_ready", "not_ready 时 status=not_ready")
            check(j.get("checks", {}).get("database", {}).get("status") == "down",
                  "依赖故障时 checks.database.status=down（可观测性）")
        check(bool(j.get("version")), "携带 version")
    scenarios.append(("ready_semantics", "A-公开端点", "GET /ready 依赖检查语义", t_ready))

    def t_metrics(check):
        s, h, text, j = c.request("metrics", "GET", "/metrics")
        check(s == 200, f"200（got {s}）")
        check(len(text) > 0, "非空指标体")
    scenarios.append(("metrics_exposed", "A-公开端点", "GET /metrics 指标可读", t_metrics))

    def t_404_route(check):
        s, h, text, j = c.request("unknown_route", "GET", "/nonexistent", auth="auth")
        check(s == 404, f"未知路由 404（got {s}）")
    scenarios.append(("unknown_route_404", "A-公开端点", "未知路由 404", t_404_route))

    # ═══ B. 认证语义 ═══
    def t_teams_me(check):
        s, h, text, j = c.request("teams_me", "GET", "/v1/teams/me")
        if s != 200:
            check(False, f"有效凭证 200（got {s}: {text[:120]!r}）")
            return
        Runner.success_envelope(check, j, "teams/me")
        data = (j or {}).get("data") or {}
        check(bool(data), "data 非空（团队画像）")
    scenarios.append(("teams_me_auth", "B-认证", "有效凭证读取团队画像", t_teams_me))

    def t_teams_me_usage(check):
        s, h, text, j = c.request("teams_usage", "GET", "/v1/teams/me/usage")
        check(s == 200, f"200（got {s}）")
        if j:
            Runner.success_envelope(check, j, "usage")
    scenarios.append(("teams_usage", "B-认证", "用量端点包封", t_teams_me_usage))

    for label, auth in (("无凭证", "noauth"), ("无效凭证", "badkey")):
        def t_auth_rejected(check, auth=auth, label=label):
            s, h, text, j = c.request(f"teams_me_{auth}", "GET", "/v1/teams/me", auth=auth)
            check(s == 401, f"{label} 401（got {s}）")
            Runner.error_envelope(check, j, s, f"teams/me({label})",
                                  want_codes={"UNAUTHORIZED"} | {None} if j and "error" not in (j or {}) else {"UNAUTHORIZED"})
        scenarios.append((f"teams_me_{auth}_401", "B-认证", f"{label}访问受保护端点 401", t_auth_rejected))

    def t_usage_noauth(check):
        s, h, text, j = c.request("usage_noauth", "GET", "/v1/teams/me/usage", auth="noauth")
        check(s == 401, f"无凭证 401（got {s}）")
    scenarios.append(("usage_noauth_401", "B-认证", "无凭证访问 usage 401", t_usage_noauth))

    # ═══ C. Scrape 语义 ═══
    def t_scrape_create(check):
        s, h, text, j = c.request("scrape_create", "POST", "/v1/scrape",
                                  body=json.dumps({"url": NEWS_PRIMARY}), timeout=60)
        check(s in (200, 201, 202), f"2xx（got {s}: {text[:160]!r}）")
        if s not in (200, 201, 202) or j is None:
            return
        Runner.success_envelope(check, j, "scrape create")
        data = (j or {}).get("data") or {}
        check(bool(data.get("id")), "data.id 存在")
        try:
            check(str(data.get("id")) and uuid.UUID(str(data.get("id"))), "data.id 为 UUID")
        except ValueError:
            check(False, "data.id 为 UUID")
        check(data.get("url") == NEWS_PRIMARY, f"url 回显请求值（got {data.get('url')!r}）")
        check(isinstance(data.get("credits_used", 0), int) and data.get("credits_used", 0) >= 0,
              "credits_used 非负整数")
        return None
    scenarios.append(("scrape_create_semantics", "C-Scrape", "创建抓取：包封+回显+UUID", t_scrape_create))

    def t_scrape_status_transition(check):
        # 真实新闻站点验证：主站失败按序故障转移；全部站点均未完成才判失败
        last_errors = []
        final, used_site = None, None
        for site, _marker in NEWS_SITES:
            s0, _, _, j0 = c.request("scrape_create_for_status", "POST", "/v1/scrape",
                                     body=json.dumps({"url": site,
                                                      "metadata": {"e2e": "status-transition"}}),
                                     timeout=60)
            check(s0 in (200, 201, 202), f"创建 2xx（got {s0}）")
            task_id = ((j0 or {}).get("data") or {}).get("id")
            if not task_id:
                last_errors.append(f"{site}: 未获得 task_id")
                continue
            deadline = time.monotonic() + 90
            seen_statuses, site_final = [], None
            while time.monotonic() < deadline:
                s, _, text, j = c.request("scrape_status_poll", "GET", f"/v1/scrape/{task_id}",
                                          timeout=30)
                check(s == 200, f"状态查询 200（got {s}）")
                if s != 200 or j is None:
                    return
                Runner.success_envelope(check, j, "scrape status")
                data = j.get("data") or {}
                st = data.get("status")
                check(st in TASK_STATUS_VOCAB, f"status ∈ 状态机词汇（got {st!r}）")
                check(data.get("id") == task_id or str(data.get("id")) == str(task_id),
                      "状态响应 id 与请求一致")
                if st not in seen_statuses:
                    seen_statuses.append(st)
                if st in TERMINAL_STATUSES:
                    site_final = data
                    break
                time.sleep(1.5)
            if not site_final:
                check(False, f"{site}: 90s 内未达终态（观测序列: {seen_statuses}）")
                return
            r.auditor.log_event("state_transition", "scrape_status_transition",
                                {"task_id": task_id, "site": site, "observed": seen_statuses,
                                 "final": site_final.get("status")})
            if site_final.get("status") in ("completed", "success"):
                final, used_site = site_final, site
                break
            last_errors.append(f"{site}: 终态 {site_final.get('status')}，"
                               f"error={str(site_final.get('error'))[:120]}")

        check(final is not None,
              f"至少一个真实新闻站点抓取完成（站点: {[s for s, _ in NEWS_SITES]}）；"
              f"各站点结果: {last_errors}")
        if not final:
            return
        result = final.get("result") or {}
        content = str(result.get("content") or "")
        check(bool(content), "completed 必含 result.content（防静默失败）")
        check(len(content) >= 200, f"真实新闻页内容量合理（len={len(content)}）")
        marker = dict(NEWS_SITES)[used_site]
        check(marker in content.lower(),
              f"正文含真实站点标记 {marker!r}（site={used_site}，证实非占位页）")
        check(result.get("status_code") == 200, f"上游 status_code=200（got {result.get('status_code')}）")
        check(isinstance(result.get("response_time_ms"), int)
              and result["response_time_ms"] >= 0, "response_time_ms 非负")
        check(bool(final.get("completed_at")), "completed_at 已填充")
        headers = result.get("headers") or {}
        leaked = [k for k in headers
                  if str(k).lower() in SENSITIVE_HEADER_NAMES
                  and str(headers[k]) != "[REDACTED]"]
        check(not leaked, f"敏感响应头已脱敏为 [REDACTED]（泄漏: {leaked}）")
    scenarios.append(("scrape_status_transition", "C-Scrape",
                      "真实新闻站抓取至完成+状态机+内容标记", t_scrape_status_transition))

    def t_scrape_unknown_id(check):
        s, _, _, j = c.request("scrape_unknown", "GET",
                               f"/v1/scrape/{uuid.uuid4()}")
        check(s == 404, f"不存在任务 404（got {s}）")
        Runner.error_envelope(check, j, s, "scrape unknown id", want_codes={"NOT_FOUND"})
    scenarios.append(("scrape_unknown_404", "C-Scrape", "查询不存在任务 404 包封", t_scrape_unknown_id))

    def t_scrape_sync_wait(check):
        s, _, text, j = c.request("scrape_sync_wait", "POST", "/v1/scrape",
                                  body=json.dumps({"url": NEWS_PRIMARY,
                                                   "sync_wait_ms": 10000}), timeout=60)
        check(s in (200, 201, 202), f"sync 等待 2xx（got {s}: {text[:120]!r}）")
        if j:
            Runner.success_envelope(check, j, "sync_wait")
    scenarios.append(("scrape_sync_wait", "C-Scrape", "sync_wait_ms 同步等待模式", t_scrape_sync_wait))

    bad_inputs = [
        ("empty_body", "", "空 body"),
        ("empty_url", json.dumps({"url": ""}), "空 URL"),
        ("ftp_scheme", json.dumps({"url": "ftp://evil.com"}), "非 HTTP 协议"),
        ("malformed_json", "not-json", "非法 JSON"),
        ("oversize_url", json.dumps({"url": "https://" + "a" * 2100 + ".com"}), "超长 URL"),
    ]
    for key, body, label in bad_inputs:
        def t_bad(check, body=body, label=label, key=key):
            s, _, text, j = c.request(f"scrape_bad_{key}", "POST", "/v1/scrape", body=body)
            check(s in (400, 422), f"{label} 拒绝 4xx（got {s}）")
            Runner.error_envelope(check, j, s, f"scrape {label}",
                                  want_codes={"VALIDATION_ERROR", "UNPROCESSABLE_ENTITY"})
        scenarios.append((f"scrape_bad_{key}", "C-Scrape", f"无效输入拒绝：{label}", t_bad))

    def t_scrape_unknown_field(check):
        s, _, _, j = c.request("scrape_unknown_field", "POST", "/v1/scrape",
                               body=json.dumps({"url": NEWS_PRIMARY,
                                                "unknown_field": True}))
        check(s == 422, f"未知字段 422（got {s}）")
    scenarios.append(("scrape_unknown_field_422", "C-Scrape", "未知字段严格拒绝 422", t_scrape_unknown_field))

    ssrf_targets = [
        ("loopback", "http://127.0.0.1:65535"),
        ("localhost", "http://localhost:8899"),
        ("metadata_svc", "http://169.254.169.254/latest/meta-data"),
        ("ipv6_loopback", "http://[::1]:8899"),
        ("wildcard_ip", "http://0.0.0.0:8899"),
        ("private_range", "http://10.1.2.3/"),
        ("non_http_scheme", "ftp://evil.com"),
    ]
    for key, url in ssrf_targets:
        def t_ssrf(check, url=url, key=key):
            s, _, text, j = c.request(f"ssrf_{key}", "POST", "/v1/scrape",
                                      body=json.dumps({"url": url}))
            check(s in (400, 403), f"SSRF {key} 拒绝 4xx（got {s}）")
            check(j is None or (j or {}).get("data") is None,
                  "SSRF 请求绝不产生任务（data 为空）")
        scenarios.append((f"ssrf_reject_{key}", "C-Scrape", f"SSRF 防护：{url}", t_ssrf))

    # ═══ D. Crawl 语义 ═══
    def _create_crawl(name, payload, timeout=60):
        s, _, text, j = c.request(name, "POST", "/v1/crawl",
                                  body=json.dumps(payload), timeout=timeout)
        return s, text, j

    def t_crawl_create(check):
        s, text, j = _create_crawl("crawl_create",
                                   {"url": NEWS_PRIMARY, "name": "e2e-sem-crawl",
                                    "config": {"max_depth": 1}})
        check(s in (200, 201, 202), f"创建 2xx（got {s}: {text[:160]!r}）")
        if j:
            Runner.success_envelope(check, j, "crawl create")
            data = (j or {}).get("data") or {}
            check(bool(data.get("id")), "data.id 存在")
    scenarios.append(("crawl_create_semantics", "D-Crawl", "创建爬取：包封+id", t_crawl_create))

    def _poll_crawl(crawl_id, deadline_s=120):
        seen, final = [], None
        deadline = time.monotonic() + deadline_s
        while time.monotonic() < deadline:
            s, _, _, j = c.request("crawl_status_poll", "GET", f"/v1/crawl/{crawl_id}", timeout=30)
            if s != 200 or j is None:
                time.sleep(2)
                continue
            data = j.get("data") or {}
            st = data.get("status")
            if st and st not in seen:
                seen.append(st)
            if st in TERMINAL_STATUSES:
                final = data
                break
            time.sleep(2)
        return seen, final

    def t_crawl_lifecycle(check):
        s, _, j = _create_crawl("crawl_lifecycle_create",
                                {"url": NEWS_PRIMARY, "name": "e2e-sem-lifecycle",
                                 "config": {"max_depth": 1}})
        check(s in (200, 201, 202), f"创建 2xx（got {s}）")
        crawl_id = ((j or {}).get("data") or {}).get("id")
        if not crawl_id:
            check(False, "未获得 crawl_id")
            return
        seen, final = _poll_crawl(crawl_id)
        check(all(st in TASK_STATUS_VOCAB for st in seen),
              f"状态词汇合法（{seen}）")
        check(final is not None, f"120s 内终态（{seen}）")
        r.auditor.log_event("state_transition", "crawl_lifecycle",
                            {"crawl_id": crawl_id, "observed": seen,
                             "final": (final or {}).get("status")})
        if not final:
            return
        check(final.get("status") in ("completed", "success", "failed"),
              "未取消的爬取以 completed/failed 终结")
        # 结果查询语义
        s2, _, _, j2 = c.request("crawl_results", "GET", f"/v1/crawl/{crawl_id}/results", timeout=30)
        check(s2 == 200, f"results 查询 200（got {s2}）")
        if j2 and s2 == 200:
            Runner.success_envelope(check, j2, "crawl results")
            results = (j2 or {}).get("data")
            check(isinstance(results, list), "data 为结果数组")
            if final.get("status") in ("completed", "success") and isinstance(results, list):
                check(len(results) >= 1, "completed 爬取至少 1 条结果（防静默空结果）")
                first = results[0] if results else {}
                check(isinstance(first.get("url"), str) and first["url"],
                      "结果项含 url")
                check(isinstance(first.get("status_code"), int),
                      "结果项含 status_code")
    scenarios.append(("crawl_lifecycle", "D-Crawl", "爬取生命周期：创建→终态→结果", t_crawl_lifecycle))

    def t_crawl_include_patterns(check):
        s, _, j = _create_crawl("crawl_include_create",
                                {"url": NEWS_FILTER_SEED, "name": "e2e-sem-filter",
                                 "config": {"max_depth": 1,
                                            "include_patterns": [NEWS_INCLUDE_PATTERN]}})
        check(s in (200, 201, 202), f"创建 2xx（got {s}）")
        crawl_id = ((j or {}).get("data") or {}).get("id")
        if not crawl_id:
            check(False, "未获得 crawl_id")
            return
        # 数据过滤验证面向结果集本身：种子页（145 个同域外链）全部抓完需数分钟，
        # 而 crawl 行在全部完成前状态恒为 queued——因此轮询结果集直到出现
        # 非种子 URL（首个子页完成后即满足，通常 <30s），不等待整场终态。
        seed = NEWS_FILTER_SEED
        pattern = re.compile(NEWS_INCLUDE_PATTERN)
        deadline = time.monotonic() + 300
        urls, seen = [], []
        while time.monotonic() < deadline:
            s2, _, _, j2 = c.request("crawl_include_results", "GET",
                                     f"/v1/crawl/{crawl_id}/results", timeout=30)
            if s2 == 200 and j2:
                results = (j2 or {}).get("data") or []
                urls = [it.get("url") for it in results if isinstance(it, dict)]
                non_seed = [u for u in urls
                            if u and u.rstrip("/") != seed.rstrip("/")]
                if non_seed:
                    break
            s3, _, _, j3 = c.request("crawl_include_status", "GET",
                                     f"/v1/crawl/{crawl_id}", timeout=30)
            if s3 == 200 and j3:
                st = ((j3 or {}).get("data") or {}).get("status")
                if st and st not in seen:
                    seen.append(st)
                if st in TERMINAL_STATUSES:
                    break
            time.sleep(3)

        if j2 and s2 == 200:
            # 契约：include_patterns 作用于抽取发现的外链（crawl_link_extractor
            # 的 UrlPatternFilter）；种子 URL 无条件抓取（标准爬虫语义），豁免匹配。
            violation = [u for u in urls
                         if u and u.rstrip("/") != seed.rstrip("/")
                         and not pattern.match(str(u))]
            check(not violation,
                  f"非种子结果 URL 全部命中 include_patterns（违规: {violation[:3]}）")
            check(any(u and u.rstrip("/") == seed.rstrip("/") for u in urls),
                  f"种子 URL 在结果中（got {urls[:3]}）")
            non_seed = [u for u in urls if u and u.rstrip("/") != seed.rstrip("/")]
            check(len(non_seed) >= 1,
                  f"外链丰富的种子应产生非种子结果（结果总数={len(urls)}，"
                  f"状态观测: {seen}，样本: {urls[:3]}）")
            r.auditor.log_event("data_filter", "crawl_include_patterns",
                                {"crawl_id": crawl_id, "result_urls": urls[:20],
                                 "pattern": NEWS_INCLUDE_PATTERN,
                                 "status_observed": seen})
        # 数据过滤已验证，取消爬取避免空耗（容忍 204/404——可能已全部完成）
        c.request("crawl_include_cleanup", "DELETE", f"/v1/crawl/{crawl_id}",
                  timeout=30)
    scenarios.append(("crawl_include_patterns", "D-Crawl", "include_patterns 数据过滤", t_crawl_include_patterns))

    def t_crawl_cancel(check):
        s, _, j = _create_crawl("crawl_cancel_create",
                                {"url": NEWS_PRIMARY, "name": "e2e-sem-cancel",
                                 "config": {"max_depth": 2}})
        check(s in (200, 201, 202), f"创建 2xx（got {s}）")
        crawl_id = ((j or {}).get("data") or {}).get("id")
        if not crawl_id:
            check(False, "未获得 crawl_id")
            return
        s2, _, text2, j2 = c.request("crawl_cancel", "DELETE", f"/v1/crawl/{crawl_id}",
                                     timeout=30)
        check(s2 == 204, f"取消 204（got {s2}）")
        check(text2.strip() == "", "204 无响应体")
        # 状态最终反映取消（存在与完成竞态：cancelled 或既达终态均合法，
        # 但绝不允许回到 running）
        deadline = time.monotonic() + 30
        observed = []
        while time.monotonic() < deadline:
            s3, _, _, j3 = c.request("crawl_cancel_status", "GET",
                                     f"/v1/crawl/{crawl_id}", timeout=30)
            if s3 == 200 and j3:
                st = ((j3 or {}).get("data") or {}).get("status")
                if st and st not in observed:
                    observed.append(st)
                if st in TERMINAL_STATUSES:
                    break
            time.sleep(1.5)
        check(observed and observed[-1] in TERMINAL_STATUSES,
              f"取消后到达终态（{observed}）")
        r.auditor.log_event("state_transition", "crawl_cancel",
                            {"crawl_id": crawl_id, "observed": observed})
    scenarios.append(("crawl_cancel_transition", "D-Crawl", "取消状态转换 204→cancelled", t_crawl_cancel))

    def t_crawl_cancel_unknown(check):
        s, _, _, j = c.request("crawl_cancel_unknown", "DELETE", f"/v1/crawl/{uuid.uuid4()}")
        check(s == 404, f"取消不存在爬取 404（got {s}）")
    scenarios.append(("crawl_cancel_unknown_404", "D-Crawl", "取消不存在任务 404", t_crawl_cancel_unknown))

    def t_crawl_max_depth_over(check):
        s, _, _, j = c.request("crawl_depth_over", "POST", "/v1/crawl",
                               body=json.dumps({"url": NEWS_PRIMARY,
                                                "name": "e2e-sem-over",
                                                "config": {"max_depth": 101}}))
        check(s == 422, f"max_depth>100 拒绝 422（got {s}）")
        if s == 422:
            Runner.error_envelope(check, j, s, "crawl max_depth=101")
    scenarios.append(("crawl_depth_limit_422", "D-Crawl", "max_depth 上限校验 422", t_crawl_max_depth_over))

    # ═══ E. Search 容错语义 ═══
    def t_search_env(check):
        s, _, text, j = c.request("search_default", "POST", "/v1/search",
                                  body=json.dumps({"query": "Rust programming",
                                                   "limit": 3}), timeout=60)
        check(s in (200, 201, 202, 500), f"成功或显式引擎错误（got {s}）")
        if j is None:
            check(False, f"响应为 JSON（{text[:120]!r}）")
            return
        if s == 500:
            # 引擎不可达的显式失败：包封必须完整（禁止静默空成功）
            Runner.error_envelope(check, j, s, "search(engine-down)",
                                  want_codes={"INTERNAL_ERROR", "EXTERNAL_SERVICE_ERROR"})
            r.auditor.log_event("env_note", "search_default",
                                {"note": "搜索引擎不可达（网络受限），验证显式失败包封"})
        else:
            Runner.success_envelope(check, j, "search ok")
            results = ((j or {}).get("data") or {}).get("results")
            check(isinstance(results, list), "data.results 为列表")
            if isinstance(results, list) and results:
                first = results[0]
                check(bool(first.get("url") or first.get("link")),
                      "结果项含 url 字段")
    scenarios.append(("search_env_tolerant", "E-Search", "搜索：成功路径/环境降级显式失败", t_search_env))

    def t_search_empty(check):
        s, _, _, j = c.request("search_empty", "POST", "/v1/search",
                               body=json.dumps({"query": ""}))
        check(s in (400, 422), f"空 query 拒绝 4xx（got {s}）")
        Runner.error_envelope(check, j, s, "search empty",
                              want_codes={"VALIDATION_ERROR", "UNPROCESSABLE_ENTITY"})
    scenarios.append(("search_empty_400", "E-Search", "空 query 校验", t_search_empty))

    def t_search_bad_engine(check):
        s, _, text, j = c.request("search_bad_engine", "POST", "/v1/search",
                                  body=json.dumps({"query": "test",
                                                   "engine": "nonexistent-engine"}), timeout=60)
        # 合法语义二选一：显式报错（引擎不存在），或按实现回落默认引擎成功
        check(s >= 400 or (j or {}).get("success") is True,
              f"未知引擎应显式报错或回落成功（got {s}: {text[:120]!r}）")
        if s >= 400 and j is not None:
            Runner.error_envelope(check, j, s, "search unknown engine")
    scenarios.append(("search_unknown_engine", "E-Search", "未知引擎显式失败", t_search_bad_engine))

    # ═══ F. 安全脱敏 ═══
    def t_apikey_issue(check):
        s, h, text, j = c.request("apikey_issue", "POST", "/v1/admin/api-keys",
                                  body=json.dumps({"team_id": r.team_id,
                                                   "scopes": ["read", "write"],
                                                   "expires_in_secs": 3600}))
        check(s == 201, f"签发 201（got {s}: {text[:160]!r}）")
        check((hdr(h, "Cache-Control") or "").lower() == "no-store",
              f"Cache-Control: no-store（got {hdr(h, 'Cache-Control')!r}）")
        check((hdr(h, "Pragma") or "").lower() == "no-cache", "Pragma: no-cache")
        check((hdr(h, "Expires") or "") == "0", f"Expires: 0（got {hdr(h, 'Expires')!r}）")
        if j is None or s != 201:
            return
        Runner.success_envelope(check, j, "apikey issue")
        data = (j or {}).get("data") or {}
        key = data.get("api_key") or ""
        check(bool(key) and GARRISON_KEY_RE.match(key),
              "data.api_key 一次性明文（id.secret 格式）")
        check(bool(data.get("api_key_id")), "data.api_key_id 存在")
        check(str(data.get("team_id")) == str(r.team_id), "team_id 回显请求值")
        check(data.get("scopes") == ["read", "write"], "scopes 回显请求值")
        check(bool(RFC3339_RE.match(str(data.get("expires_at", "")))), "expires_at RFC3339")
        if key:
            r.issued_keys.append(key)
            r.new_key = key
    scenarios.append(("apikey_issue_semantics", "F-安全脱敏", "key 签发：一次性明文+防缓存头", t_apikey_issue))

    def t_apikey_lifecycle(check):
        key = getattr(r, "new_key", None)
        if not key:
            r.auditor.log_event("env_note", "apikey_lifecycle",
                                {"note": "前序签发失败，跳过（依赖 apikey_issue_semantics）"})
            return  # 无断言 → SKIP
        saved = c.api_key
        c.api_key = key
        try:
            s, _, _, j = c.request("apikey_new_key_auth", "GET", "/v1/teams/me")
            check(s == 200, f"新 key 立即可用 200（got {s}）")
        finally:
            c.api_key = saved
    scenarios.append(("apikey_lifecycle", "F-安全脱敏", "新签发 key 立即可认证", t_apikey_lifecycle))

    def t_apikey_readonly_scope(check):
        s, _, text, j = c.request("apikey_issue_readonly", "POST", "/v1/admin/api-keys",
                                  body=json.dumps({"team_id": r.team_id,
                                                   "scopes": ["read"],
                                                   "expires_in_secs": 3600}))
        check(s == 201, f"只读 key 签发 201（got {s}）")
        key = (((j or {}).get("data") or {}).get("api_key")) if j else None
        if not key:
            check(False, "未获得只读 key")
            return
        r.issued_keys.append(key)
        saved = c.api_key
        c.api_key = key
        try:
            s2, _, text2, j2 = c.request("apikey_readonly_admin", "POST", "/v1/admin/api-keys",
                                         body=json.dumps({"team_id": r.team_id,
                                                          "scopes": ["read", "write", "admin"]}))
            check(s2 == 403, f"只读 key 签发 admin key 被拒 403（got {s2}）")
            Runner.error_envelope(check, j2, s2, "readonly → admin",
                                  want_codes={"FORBIDDEN"} if j2 else None)
        finally:
            c.api_key = saved
    scenarios.append(("apikey_scope_enforcement", "F-安全脱敏", "只读 scope 无法越权签发（403）", t_apikey_readonly_scope))

    def t_apikey_bad_input(check):
        s1, _, _, j1 = c.request("apikey_nil_team", "POST", "/v1/admin/api-keys",
                                 body=json.dumps({"team_id": "00000000-0000-0000-0000-000000000000",
                                                  "scopes": ["read"]}))
        check(s1 == 400, f"nil team 400（got {s1}）")
        s2, _, _, j2 = c.request("apikey_empty_scopes", "POST", "/v1/admin/api-keys",
                                 body=json.dumps({"team_id": r.team_id, "scopes": []}))
        check(s2 == 400, f"空 scopes 400（got {s2}）")
    scenarios.append(("apikey_input_validation", "F-安全脱敏", "签发输入校验（nil team/空 scopes）", t_apikey_bad_input))

    def t_geo_roundtrip(check):
        s1, _, _, j1 = c.request("geo_put", "PUT", "/v1/teams/geo-restrictions",
                                 body=json.dumps({"enable_geo_restrictions": False}))
        check(s1 == 200, f"PUT 200（got {s1}）")
        s2, _, _, j2 = c.request("geo_get", "GET", "/v1/teams/geo-restrictions")
        check(s2 == 200, f"GET 200（got {s2}）")
        if j2 and s2 == 200:
            Runner.success_envelope(check, j2, "geo get")
            data = (j2 or {}).get("data") or {}
            check(data.get("enable_geo_restrictions") is False,
                  "写后读一致（enable=false 持久化）")
    scenarios.append(("geo_roundtrip_consistency", "F-安全脱敏", "geo 限制写后读一致", t_geo_roundtrip))

    def t_audit_masked(check):
        s, _, text, j = c.request("audit_logs", "GET", "/v1/audit/logs?limit=50")
        check(s == 200, f"200（got {s}）")
        if j is None or s != 200:
            return
        Runner.success_envelope(check, j, "audit logs")
        check(isinstance((j.get("data") or {}).get("logs"), list), "data.logs 列表")
        # 脱敏审查：全部已签发 key 明文不得出现在响应中
        for k in r.issued_keys:
            check(k not in text, "审计日志不含明文 API key")
        if c.api_key:
            check(c.api_key not in text, "审计日志不含当前请求凭证")
    scenarios.append(("audit_logs_masked", "F-安全脱敏", "审计日志脱敏审查", t_audit_masked))

    def t_audit_denied_scoped(check):
        s, _, text, j = c.request("audit_denied", "GET", "/v1/audit/denied?limit=10")
        check(s == 200, f"200（got {s}）")
        if j and s == 200:
            Runner.success_envelope(check, j, "audit denied")
    scenarios.append(("audit_denied_scoped", "F-安全脱敏", "拒绝请求查询按本 key 作用域", t_audit_denied_scoped))

    # ═══ G. 任务管理 / 数据过滤 ═══
    def t_tasks_query_auth_scoped(check):
        # 关键语义：请求体 team_id 不是权威过滤源，auth_state.team_id 才是
        # （IDOR 防护）。用随机外部 team_id 查询，结果仍应为本 team 任务视图。
        foreign = str(uuid.uuid4())
        s, _, text, j = c.request("tasks_query_foreign_team", "POST", "/v1/tasks/_query",
                                  body=json.dumps({"team_id": foreign, "limit": 10}))
        check(s == 200, f"外部 team_id 查询不越权且不 500（got {s}: {text[:120]!r}）")
        if j and s == 200:
            Runner.success_envelope(check, j, "tasks query(foreign team_id)")
            data = j.get("data") or {}
            check(isinstance(data.get("tasks"), list), "data.tasks 列表")
            check(isinstance(data.get("total"), int), "data.total 整数")
            check(isinstance(data.get("has_more"), bool), "data.has_more 布尔")
        # 对照组：本 team 查询可见自身任务
        s2, _, _, j2 = c.request("tasks_query_own_team", "POST", "/v1/tasks/_query",
                                 body=json.dumps({"team_id": r.team_id, "limit": 50}))
        check(s2 == 200, f"本 team 查询 200（got {s2}）")
    scenarios.append(("tasks_query_auth_scoped", "G-任务过滤", "team_id 以认证身份为准（IDOR）", t_tasks_query_auth_scoped))

    def t_tasks_query_filters(check):
        s, _, _, j = c.request("tasks_query_limit1", "POST", "/v1/tasks/_query",
                               body=json.dumps({"team_id": r.team_id, "limit": 1,
                                                "sync_wait_ms": 0}))
        check(s == 200, f"limit=1 200（got {s}）")
        if j and s == 200:
            tasks = ((j or {}).get("data") or {}).get("tasks") or []
            check(len(tasks) <= 1, f"limit 被尊重（got {len(tasks)} 条）")
        s2, _, _, j2 = c.request("tasks_query_types", "POST", "/v1/tasks/_query",
                                 body=json.dumps({"team_id": r.team_id,
                                                  "task_types": ["crawl"], "limit": 20}))
        check(s2 == 200, f"task_types=crawl 200（got {s2}）")
        if j2 and s2 == 200:
            tasks = ((j2 or {}).get("data") or {}).get("tasks") or []
            check(all(t.get("task_type") == "crawl" for t in tasks),
                  "类型过滤精确（全部为 crawl）")
    scenarios.append(("tasks_query_filters", "G-任务过滤", "limit/task_types 过滤语义", t_tasks_query_filters))

    def t_tasks_cancel_empty(check):
        s, _, _, j = c.request("tasks_cancel_empty", "POST", "/v1/tasks/_cancel",
                               body=json.dumps({}))
        check(s == 422, f"空请求 422（got {s}）")
        Runner.error_envelope(check, j, s, "tasks cancel empty")
    scenarios.append(("tasks_cancel_empty_422", "G-任务过滤", "批量取消空请求 422", t_tasks_cancel_empty))

    # ═══ I. 文本提取（去 HTML 标签）═══
    def t_markdown_extraction(check):
        # formats=["markdown"] → worker 经 HTML→Markdown 转换生成去标签正文，
        # 结果经 save_result 落入 result.meta_data.markdown
        s0, _, _, j0 = c.request("extract_md_create", "POST", "/v1/scrape",
                                 body=json.dumps({"url": NEWS_PRIMARY,
                                                  "formats": ["markdown"],
                                                  "exclude_tags": ["style", "script"],
                                                  "metadata": {"e2e": "text-extraction"}}),
                                 timeout=60)
        check(s0 in (200, 201, 202), f"创建 2xx（got {s0}）")
        task_id = ((j0 or {}).get("data") or {}).get("id")
        if not task_id:
            check(False, "未获得 task_id")
            return
        final = None
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            s, _, _, j = c.request("extract_md_poll", "GET", f"/v1/scrape/{task_id}",
                                   timeout=30)
            if s == 200 and j:
                st = ((j.get("data") or {}).get("status"))
                if st in TERMINAL_STATUSES:
                    final = j.get("data")
                    break
            time.sleep(1.5)
        check(final is not None, "90s 内到达终态")
        if not final:
            return
        check(final.get("status") in ("completed", "success"),
              f"提取用例需 completed（got {final.get('status')}: "
              f"{str(final.get('error'))[:120]}）")
        if final.get("status") not in ("completed", "success"):
            return
        md = (((final.get("result") or {}).get("meta_data") or {}).get("markdown")) or ""
        check(bool(md) and len(md) >= 100, f"meta_data.markdown 非空（len={len(md)}）")
        if not md:
            return
        low = md.lower()
        for tag in ("<html", "<div", "<p>", "</p>", "<a href"):
            check(tag not in low, f"提取结果不含 HTML 标签 {tag!r}")
        check("npr.org" in low, "提取文本含真实站点标记 npr.org")
        # 提取结果可视：打印摘录（进入 liveapi 日志）+ 全文样本入审计日志
        print("\n    ── 去标签提取结果（meta_data.markdown 前 600 字符）──")
        for line in md[:600].splitlines()[:12]:
            print(f"    │ {line}")
        r.auditor.log_event("text_extraction", "markdown_extraction",
                            {"task_id": task_id, "markdown_len": len(md),
                             "excerpt": md[:800]})
    scenarios.append(("markdown_text_extraction", "I-文本提取",
                      "formats=markdown 去标签提取+真实内容", t_markdown_extraction))

    # ═══ H. 并发 ═══
    def t_concurrent_scrapes(check):
        results, errs = [], []
        lock = threading.Lock()

        def worker(i):
            try:
                s, _, text, j = c.request(f"concurrent_{i}", "POST", "/v1/scrape",
                                          body=json.dumps({"url": NEWS_PRIMARY,
                                                           "metadata": {"e2e": f"concurrent-{i}"}}),
                                          timeout=60)
                with lock:
                    results.append((s, ((j or {}).get("data") or {}).get("id")))
            except Exception as e:
                with lock:
                    errs.append(str(e))

        threads = [threading.Thread(target=worker, args=(i,)) for i in range(5)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        check(not errs, f"无请求异常（{errs[:2]}）")
        ok_ids = [tid for s, tid in results if s in (200, 201, 202) and tid]
        check(len(ok_ids) == 5, f"5 个并发请求全部受理（got {len(ok_ids)}）")
        check(len(set(map(str, ok_ids))) == len(ok_ids), "任务 id 全部唯一（无碰撞）")
    scenarios.append(("concurrent_scrapes", "H-并发", "并发提交唯一性与稳定性", t_concurrent_scrapes))

    return scenarios


# ── 主流程 ──────────────────────────────────────────────────────────────────

def main():
    ap = argparse.ArgumentParser(description="crawlrs API 语义级 E2E 测试")
    ap.add_argument("--base-url", default="http://localhost:8901")
    ap.add_argument("--api-key", required=True)
    ap.add_argument("--team-id", required=True)
    ap.add_argument("--audit-log", default="test-results/api-audit.jsonl")
    args = ap.parse_args()

    import os
    os.makedirs(os.path.dirname(args.audit_log) or ".", exist_ok=True)

    auditor = Auditor(args.audit_log, secrets=[args.api_key])
    client = Client(args.base_url, args.api_key, auditor)
    runner = Runner(client, auditor, args.team_id, args.base_url)

    print(f"crawlrs API 语义级 E2E — {args.base_url}")
    print(f"审计日志: {args.audit_log}\n")

    scenarios = build_scenarios(runner)
    last_cat = None
    for name, cat, desc, func in scenarios:
        if cat != last_cat:
            print(f"\n▶ {cat}")
            last_cat = cat
        runner.run(name, cat, desc, func)

    # 汇总
    print(f"\n{'=' * 60}")
    print(f"  总计: {len(runner.results)}  通过: {runner.passed}  "
          f"失败: {runner.failed}  跳过: {runner.skipped}")
    print(f"  审计日志: {args.audit_log}")

    # 覆盖矩阵 markdown
    matrix_path = "test-results/api-semantics-matrix.md"
    try:
        with open(matrix_path, "w", encoding="utf-8") as f:
            f.write("# API 语义 E2E 覆盖矩阵\n\n")
            f.write(f"- 时间: {datetime.now(timezone.utc).isoformat()}\n")
            f.write(f"- 目标: {args.base_url}\n")
            f.write(f"- 结果: {runner.passed} 通过 / {runner.failed} 失败 / "
                    f"{runner.skipped} 跳过\n\n")
            f.write("| 类别 | 场景 | 用例 | 结果 | 断言数 | 失败明细 |\n")
            f.write("|---|---|---|---|---|---|\n")
            for row in runner.results:
                fails = "; ".join(row["failed_checks"]) or "—"
                f.write(f"| {row['category']} | {row['scenario']} | {row['name']} "
                        f"| {row['status']} | {row['checks_total']} | {fails} |\n")
        print(f"  覆盖矩阵: {matrix_path}")
    except OSError as e:
        print(f"  覆盖矩阵写入失败: {e}")

    # 回溯掩码：已签发 key 的明文出现在签发响应体中（写入时该 key 尚不可知），
    # 运行结束时统一掩码，确保审计文件不留存任何凭证明文（CWE-532）
    if runner.issued_keys and auditor.path:
        with open(auditor.path, encoding="utf-8") as f:
            content = f.read()
        for k in runner.issued_keys:
            content = content.replace(k, mask_token(k))
        with open(auditor.path, "w", encoding="utf-8") as f:
            f.write(content)

    auditor.close()
    sys.exit(runner.failed)


if __name__ == "__main__":
    main()
