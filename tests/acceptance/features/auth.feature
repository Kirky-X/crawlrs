# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-acceptance-003/004：认证与 api-key 签发验收（正常流 + 异常矩阵）
# 契约：POST /v1/admin/api-keys 成功返回 201（Created）。

Feature: Authentication and API key issuance
  garrison 认证：签发、无效密钥、畸形格式、越权

  Scenario: Sign a regular API key via admin endpoint
    Given an admin API key
    When I sign a regular API key with scopes "read,write"
    Then the response status is 201
    And the response JSON field "success" is true
    And the response JSON field "data.api_key" is a non-empty string

  Scenario: Signed key authenticates and resolves its team
    Given a regular API key signed via admin endpoint
    When I GET "/v1/teams/me"
    Then the response status is 200
    And the response JSON field "success" is true

  Scenario: Missing Authorization header is rejected
    Given no API key
    When I GET "/v1/teams/me"
    Then the response status is 401

  Scenario: Invalid key is rejected
    Given an invalid API key
    When I GET "/v1/teams/me"
    Then the response status is 401

  Scenario: Malformed Authorization format is rejected
    When I GET "/v1/teams/me" with raw Authorization "not-bearer-format"
    Then the response status is 401

  Scenario: Regular key cannot access admin endpoint
    Given a regular API key signed via admin endpoint
    When I sign a regular API key with scopes "read"
    Then the response status is 403
