# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE in the project root for full license information.

# R-acceptance-003/004：search 验收
# 契约注记（离线约束）：
# - 主引擎（baidu/bing/sogou）端点在 src/search/client/{baidu,bing,sogou}.rs 内硬编码，
#   无配置注入缝；`[search.fallback]` 的 exa/parallel/tavily 有 endpoint 配置项但
#   未在 SearchClient 注册（engine="tavily" 落 NoEngineAvailable）。故「真实搜索结果」
#   场景无法离线覆盖，标记 @requires-internet，CI 以 CRAWLRS_ACCEPTANCE_OFFLINE=1 跳过。

Feature: Search
  统一搜索端点正常流与异常矩阵

  @requires-internet
  @external
  Scenario: Search returns results
    # 真实搜索引擎端点硬编码（Bing 等），无法以 wiremock 拦截——标记 @external，
    # 默认以 `--tags "not @external"` 跳过；手动联网验证时去掉该过滤运行。
    Given an admin API key
    When I search for "crawlrs acceptance" at "/v1/search"
    Then the response status is 200
    And the response JSON field "success" is true
    And the response JSON pointer "/data/results" is a non-empty array

  Scenario: Empty query is rejected with 400
    Given an admin API key
    When I search for "" at "/v1/search"
    Then the response status is 400

  Scenario: Unregistered engine name does not silently fall back
    # 契约：engine="tavily" 有枚举映射但未注册实现 → SearchError::NoEngineAvailable，
    # 不静默回退到默认引擎（静默回退会掩盖配置错误）。
    Given an admin API key
    When I search for "crawlrs" at "/v1/search" with engine "tavily"
    Then the response status is one of "400,422,500,502"

  Scenario: Missing query field is rejected
    Given an admin API key
    When I POST "/v1/search" with an empty body
    Then the response status is 422
