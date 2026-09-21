# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-acceptance-003/004：crawl 全生命周期验收
# 目标站为 support 的 mock 三页互链站点（/page_a → /page_b → /page_c）。

Feature: Crawl lifecycle
  crawl 任务创建→执行→取结果→取消与异常矩阵

  Scenario: Create and complete a crawl task
    Given an admin API key
    When I create a crawl at "/v1/crawl" for "{mock_base}/page_a" with max depth 1
    Then the response status is 200 or 201
    When I wait for crawl "/v1/crawl" to complete within 60 seconds
    And I GET the task detail at "/v1/crawl"
    Then the response status is 200
    And the response JSON field "success" is true
    And the response JSON field "data.status" is "completed"

  Scenario: Crawl results are retrievable after completion
    Given a crawl of "{mock_base}/page_a" completed
    When I GET template "/v1/crawl/{task_id}/results"
    Then the response status is 200
    And the response JSON field "success" is true

  Scenario: Cancel a running crawl task
    # 契约：取消成功返回 204 No Content（无响应体）
    Given an admin API key
    When I create a crawl at "/v1/crawl" for "{mock_base}/page_a" with max depth 2
    Then the response status is 200 or 201
    When I DELETE template "/v1/crawl/{task_id}"
    Then the response status is 204

  Scenario: Invalid crawl URL is rejected
    # 契约：与 scrape 一致，SSRF 静态校验先于 DTO validator，无效 URL → 400
    # VALIDATION_ERROR（"Invalid URL 'not-a-url': relative URL without a base"）
    Given an admin API key
    When I create a crawl at "/v1/crawl" for "not-a-url" with max depth 1
    Then the response status is 400

  Scenario: Unknown crawl id returns 404
    Given an admin API key
    When I GET "/v1/crawl/00000000-0000-0000-0000-000000000000"
    Then the response status is 404

  Scenario: Results for unknown crawl id returns 404
    Given an admin API key
    When I GET "/v1/crawl/00000000-0000-0000-0000-000000000000/results"
    Then the response status is 404
