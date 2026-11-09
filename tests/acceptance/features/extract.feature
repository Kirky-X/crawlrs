# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE in the project root for full license information.

# R-acceptance-003/004：extract 验收（同步型端点）
# 契约注记：
# - 请求体为 {"urls": [...]}（ExtractRequestDto），步骤内单 URL 包成数组；
# - 无效 URL 由 DTO validator 先拒绝（http/https scheme 校验）→ 422；
#   sync_wait_ms 超限由 handler 廉价校验 → 400。

Feature: Extract
  HTML 数据提取端点正常流与异常矩阵

  Scenario: Extract data from a mock page
    # 契约：POST /v1/extract 创建异步任务（默认 5s 同步等待，超窗 202 Accepted）；
    # 结果经 GET /v1/scrape/{id} 轮询（任务通用查询）
    Given an admin API key
    When I extract url "{mock_base}/page" at "/v1/extract"
    Then the response status is one of "200,201,202"
    And the response JSON field "data.status" is "pending"
    And the response JSON field "data.id" is a non-empty string

  Scenario: Invalid extract URL is rejected with 422
    Given an admin API key
    When I extract url "not-a-url" at "/v1/extract"
    Then the response status is 422

  Scenario: Missing urls field is rejected
    Given an admin API key
    When I POST "/v1/extract" with an empty body
    Then the response status is 422

  Scenario: sync_wait_ms above the maximum is rejected with 400
    # 契约：sync_wait_ms > 30000 由 handler 在 DTO/SSRF 之前手动校验 → 400
    Given an admin API key
    When I extract url "{mock_base}/page" at "/v1/extract" with sync wait ms 30001
    Then the response status is 400
