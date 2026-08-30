# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-acceptance-003/004：search 验收（搜索引擎经 support 的 mock 搜索引擎响应）

Feature: Search
  统一搜索端点正常流与异常矩阵

  Scenario: Search returns results
    Given an admin API key
    When I search for "crawlrs acceptance" at "/v1/search"
    Then the response status is 200
    And the response JSON field "success" is true
    And the response JSON pointer "/data/results" is a non-empty array

  Scenario: Empty query is rejected with 400
    Given an admin API key
    When I search for "" at "/v1/search"
    Then the response status is 400
