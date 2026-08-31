# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-map-001~004：/v1/map 验收
# sitemap 由 support 的 4 个专用 wiremock 站提供：
#   {sitemap_base}  → 3 loc 根 sitemap（含 1 个 */blog/*）
#   {index_base}    → index（2 子 sitemap 共 5 loc）
#   {site404}       → 任意 GET 404（缺 sitemap）
#   {site500}       → 任意 GET 500（目标不可达）

Feature: Map endpoint
  站点 sitemap URL 发现：解析/递归/过滤/截断/错误映射

  Scenario: Discover URLs from a sitemap with 3 locs
    Given an admin API key
    When I map "{sitemap_base}/page"
    Then the response status is 200 or 201
    And the response JSON field "success" is true
    And the response JSON pointer "/data/links" is an array of length 3

  Scenario: Map follows a sitemap index one level deep
    Given an admin API key
    When I map "{index_base}/page"
    Then the response status is 200 or 201
    And the response JSON pointer "/data/links" is an array of length 5

  Scenario: Map with include pattern filters links
    Given an admin API key
    When I map "{sitemap_base}/page" including "*/blog/*"
    Then the response status is 200
    And the response JSON pointer "/data/links" is an array of length 2

  Scenario: Map with limit truncates links
    Given an admin API key
    When I map "{sitemap_base}/page" with limit 2
    Then the response status is 200
    And the response JSON pointer "/data/links" is an array of length 2

  Scenario: Missing sitemap returns empty links
    Given an admin API key
    When I map "{site404}/page"
    Then the response status is 200 or 201
    And the response JSON pointer "/data/links" is an array of length 0

  Scenario: Target failure maps to 502 MAP_TARGET_UNREACHABLE
    Given an admin API key
    When I map "{site500}/page"
    Then the response status is 502
    And the response JSON field "error.code" is "MAP_TARGET_UNREACHABLE"

  Scenario: Invalid map URL is rejected
    # 契约：map 的 DTO validator 校验（"url: URL must start with http://"）→ 422
    Given an admin API key
    When I map "not-a-url"
    Then the response status is 422

  Scenario: Unauthenticated map request is rejected
    Given no API key
    When I map "{sitemap_base}/page"
    Then the response status is 401
