# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE in the project root for full license information.

# R-acceptance-003/004：extract 验收（同步型端点）
# 契约注记：
# - 请求体为 {"urls": [...]}（ExtractRequestDto），步骤内单 URL 包成数组；
# - 无效 URL 与 scrape/crawl 一致，SSRF 静态校验先于 DTO validator → 400。

Feature: Extract
  HTML 数据提取端点正常流与异常矩阵

  Scenario: Extract data from a mock page
    # 契约：POST /v1/extract 创建异步任务，返回 {id, status:"pending"}（200/201）；
    # 结果经 GET /v1/scrape/{id} 轮询（任务通用查询）
    Given an admin API key
    When I extract url "{mock_base}/page" at "/v1/extract"
    Then the response status is 200 or 201
    And the response JSON field "data.status" is "pending"
    And the response JSON field "data.id" is a non-empty string

  Scenario: Invalid extract URL is rejected with 400
    Given an admin API key
    When I extract url "not-a-url" at "/v1/extract"
    Then the response status is 400

  Scenario: Missing urls field is rejected
    Given an admin API key
    When I POST "/v1/extract" with an empty body
    Then the response status is 422
