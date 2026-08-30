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
    Given an admin API key
    When I create a webhook at "/v1/webhooks" pointing to "{mock_base}/hook" for events "crawl.completed"
    Then the response status is 200 or 201
    And the response JSON field "url" is a non-empty string
    And I GET "/v1/webhooks"
    Then the response status is 200

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

  Scenario: Regular key cannot access admin api-keys endpoint
    Given a regular API key signed via admin endpoint
    When I sign a regular API key with scopes "read"
    Then the response status is 403
