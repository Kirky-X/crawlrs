# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-acceptance-003：公开端点正常流验收

Feature: Public endpoints
  公开端点（健康检查/就绪/版本号）对未认证调用方可用

  Scenario: Health check reports healthy
    When I GET "/health"
    Then the response status is 200
    And the response JSON field "status" is "healthy"

  Scenario: Readiness check succeeds
    When I GET "/ready"
    Then the response status is 200

  Scenario: Version is reported
    When I GET "/v1/version"
    Then the response status is 200
