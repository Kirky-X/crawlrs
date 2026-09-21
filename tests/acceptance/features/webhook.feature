# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# R-whv-001/002：Webhook 投递端到端验收（创建→任务完成→投递→验签）

Feature: Webhook delivery end-to-end
  任务完成事件经 webhook worker 签名投递到接收方

  Scenario: Signed webhook delivery after scrape completion
    Given an admin API key
    When I create a scrape at "/v1/scrape" for "{mock_base}/page" with webhook "{mock_base}/hook"
    Then the response status is 201
    When I wait for scrape "/v1/scrape" to complete within 30 seconds
    And I wait at most 20 seconds for a webhook delivery to "/hook"
    Then the delivery has non-empty headers "webhook-id,webhook-timestamp,webhook-signature"
    And the delivery signature verifies with the test secret
    And the delivery body JSON contains the task id from context "task_id"
