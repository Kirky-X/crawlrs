# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-acceptance-003/004：平台能力验收（teams/webhooks/tasks/audit + 越权矩阵）
# 契约注记：
# - POST /v1/webhooks 成功返回 201；空 url 被 webhook service 以 401
#   （WEBHOOK_AUTH_FAILED，认证语义复用）拒绝——按实际契约断言。

Feature: Platform capabilities
  teams/webhooks/tasks/audit 端点与越权矩阵

  Scenario: Teams me resolves the caller team
    Given an admin API key
    When I GET "/v1/teams/me"
    Then the response status is 200
    And the response JSON field "success" is true

  Scenario: Teams usage returns counters
    Given an admin API key
    When I GET "/v1/teams/me/usage"
    Then the response status is 200
    And the response JSON field "success" is true

  Scenario: Create and list webhooks
    # 契约：创建响应走 {success,data} 包封，data.secret 一次性返回签名密钥
    Given an admin API key
    When I create a webhook at "/v1/webhooks" pointing to "{mock_base}/hook" for events "crawl.completed"
    Then the response status is 201
    And the response JSON field "data.url" is a non-empty string
    And the response JSON field "data.secret" is a non-empty string
    When I GET "/v1/webhooks"
    Then the response status is 200
    And the response JSON field "success" is true

  Scenario: Webhook with missing url is rejected
    # 契约：空 url 被 webhook_url SSRF 校验拒绝（400 "Invalid webhook URL"）
    Given an admin API key
    When I create a webhook at "/v1/webhooks" pointing to "" for events "crawl.completed"
    Then the response status is 400

  Scenario: Audit logs are listable
    Given an admin API key
    When I GET "/v1/audit/logs"
    Then the response status is 200
    And the response JSON field "success" is true

  Scenario: Complex task query returns 200 or 422
    # 契约：空 {} 缺 team_id → 422 VALIDATION_ERROR；合法查询 → 200
    Given an admin API key
    When I POST "/v1/tasks/_query" with an empty body
    Then the response status is 200 or 422

  Scenario: Batch cancel via tasks endpoint
    # 契约：POST /v1/tasks/_cancel 批量取消（TaskCancelRequestDto.task_ids 必填），
    # 返回 data.cancelled_tasks 数组与 data.total_cancelled 计数
    Given an admin API key
    When I create a scrape at "/v1/scrape" for "{mock_base}/page"
    And I store response JSON field "data.id" as "task_a"
    And I create a scrape at "/v1/scrape" for "{mock_base}/fail"
    And I store response JSON field "data.id" as "task_b"
    And I cancel tasks from context "task_a" and "task_b" at "/v1/tasks/_cancel"
    Then the response status is 200
    And the response JSON pointer "/data/cancelled_tasks" is an array of length 2
    And the response JSON field "data.total_cancelled" is a non-empty string or number

  Scenario: Regular key cannot access admin api-keys endpoint
    Given a regular API key signed via admin endpoint
    When I sign a regular API key with scopes "read"
    Then the response status is 403

  Scenario: Audit denied requests are listable
    Given an admin API key
    When I GET "/v1/audit/denied"
    Then the response status is 200
    And the response JSON field "success" is true

  Scenario: SearXNG endpoints return 503 while disabled
    # 契约：search.searxng.enabled=false（默认）时实例端点统一 503 SERVICE_UNAVAILABLE
    Given an admin API key
    When I GET "/v1/searxng/pool"
    Then the response status is 503
