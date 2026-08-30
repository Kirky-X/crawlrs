# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-acceptance-003/004：extract 验收（同步型端点）

Feature: Extract
  HTML 数据提取端点正常流与异常矩阵

  Scenario: Extract data from a mock page
    Given an admin API key
    When I extract urls ["{mock_base}/page"] at "/v1/extract"
    Then the response status is 200
    And the response JSON field "success" is true

  Scenario: Invalid extract URL is rejected with 422
    Given an admin API key
    When I extract from "not-a-url"
    Then the response status is 422
