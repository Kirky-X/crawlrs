# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-sdk-001/002：SDK 端点验收（/api/v1/sdk/*，受 auth 保护）
# 契约注记：sdk/search 正常流依赖硬编码引擎端点 → @external（同 /v1/search）。

Feature: SDK endpoints
  SDK 编程接口：任务创建/搜索与认证

  Scenario: Create a scrape task via SDK
    Given an admin API key
    When I create a scrape at "/api/v1/sdk/scrape" for "{mock_base}/page"
    Then the response status is 200 or 201
    And the response JSON field "id" is a non-empty string

  Scenario: Create a scrape task via SDK tasks endpoint
    Given an admin API key
    When I create a sdk task at "/api/v1/sdk/tasks" for "{mock_base}/page" with type "scrape"
    Then the response status is 200 or 201
    And the response JSON field "status" is a non-empty string

  Scenario: Create a crawl via SDK
    # 契约：SdkCreateCrawlRequest = {name, url, seed_url} 全必填
    Given an admin API key
    When I create a sdk crawl at "/api/v1/sdk/crawl" named "acc-crawl" with url "{mock_base}/page_a" seed "{mock_base}/page_a"
    Then the response status is 200 or 201

  Scenario: SDK scrape task completes end to end
    Given an admin API key
    When I create a scrape at "/api/v1/sdk/scrape" for "{mock_base}/page"
    When I wait for scrape "/v1/scrape" to complete within 30 seconds
    And I GET the task detail at "/v1/scrape"
    Then the response JSON field "data.status" is "completed"

  Scenario: SDK search with empty query is rejected with 400
    Given an admin API key
    When I search for "" at "/api/v1/sdk/search"
    Then the response status is 400

  Scenario: SDK search without query field is rejected with 422
    Given an admin API key
    When I POST "/api/v1/sdk/search" with an empty body
    Then the response status is 422

  Scenario: Unauthenticated SDK access is rejected
    Given no API key
    When I GET "/api/v1/sdk/search"
    Then the response status is 401

  @external
  Scenario: SDK search returns results (requires internet)
    Given an admin API key
    When I search for "crawlrs acceptance" at "/api/v1/sdk/search"
    Then the response status is 200
