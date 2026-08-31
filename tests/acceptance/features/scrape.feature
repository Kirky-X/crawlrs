# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-acceptance-003/004：scrape 全生命周期验收（正常流 + 异常矩阵）
# 目标站为 wiremock 本地 mock（http://127.0.0.1:{port}/page），由 support 的
# mock_target_server 提供（feature 内以 {mock_base} 模板引用 base URL）。

Feature: Scrape lifecycle
  scrape 任务创建→执行→取结果与异常矩阵

  Scenario: Create and complete a scrape task
    # 契约：任务创建返回 201 Created（与 search/map 同步端点区分）
    Given an admin API key
    When I create a scrape at "/v1/scrape" for "{mock_base}/page"
    Then the response status is 201
    When I wait for scrape "/v1/scrape" to complete within 30 seconds
    And I GET the task detail at "/v1/scrape"
    Then the response status is 200
    And the response JSON field "success" is true
    And the response body contains "acceptance-marker-content"

  Scenario: Invalid URL is rejected
    # 契约：无效 URL 在 SSRF 静态校验阶段即被拒（400），早于 DTO validator 的 422
    Given an admin API key
    When I create a scrape at "/v1/scrape" for "not-a-url"
    Then the response status is 400

  Scenario: Missing url field is rejected
    Given an admin API key
    When I POST "/v1/scrape" with an empty body
    Then the response status is 422

  Scenario: Unknown task id returns 404
    Given an admin API key
    When I GET "/v1/scrape/00000000-0000-0000-0000-000000000000"
    Then the response status is 404

  Scenario: Target server error still completes the task
    # 契约：引擎拿到 HTTP 500 响应体即算任务完成（status_code 记录在结果中），
    # 任务不因目标站错误而 failed
    Given an admin API key
    When I create a scrape at "/v1/scrape" for "{mock_base}/fail"
    Then the response status is 201
    When I wait for scrape "/v1/scrape" to complete within 30 seconds
    And I GET the task detail at "/v1/scrape"
    Then the response status is 200
    And the response JSON field "data.status" is "completed"
