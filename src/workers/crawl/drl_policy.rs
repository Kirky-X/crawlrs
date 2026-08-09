// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in project root for full license information.

//! DRL (Deep Reinforcement Learning) 自适应爬取策略
//!
//! 使用预训练的 ONNX 模型根据爬取状态预测最优动作：
//! - URL 优先级调整
//! - 并发度变化
//! - 引擎选择
//! - 重试决策
//!
//! 模型通过 ONNX Runtime 推理。不可用时退化为启发式规则。

use serde::{Deserialize, Serialize};

/// 爬取状态（DRL 模型输入）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlState {
    /// 当前队列深度（待处理 URL 数）
    pub queue_depth: u32,
    /// 域名平均响应时间（毫秒）
    pub domain_response_time_avg: f32,
    /// 爬取成功率（0.0-1.0）
    pub success_rate: f32,
    /// 内存压力（0.0-1.0，当前使用/总量）
    pub memory_pressure: f32,
    /// 剩余预算比例（0.0-1.0）
    pub budget_remaining: f32,
}

impl CrawlState {
    /// 将状态转为特征向量（用于模型推理）
    pub fn to_feature_vector(&self) -> Vec<f32> {
        vec![
            self.queue_depth as f32 / 1000.0, // 归一化
            self.domain_response_time_avg / 10000.0,
            self.success_rate,
            self.memory_pressure,
            self.budget_remaining,
        ]
    }

    /// 验证状态有效性
    pub fn validate(&self) -> Result<(), String> {
        if self.success_rate < 0.0 || self.success_rate > 1.0 {
            return Err(format!("success_rate out of range: {}", self.success_rate));
        }
        if self.memory_pressure < 0.0 || self.memory_pressure > 1.0 {
            return Err(format!(
                "memory_pressure out of range: {}",
                self.memory_pressure
            ));
        }
        if self.budget_remaining < 0.0 || self.budget_remaining > 1.0 {
            return Err(format!(
                "budget_remaining out of range: {}",
                self.budget_remaining
            ));
        }
        Ok(())
    }
}

impl Default for CrawlState {
    fn default() -> Self {
        Self {
            queue_depth: 0,
            domain_response_time_avg: 0.0,
            success_rate: 1.0,
            memory_pressure: 0.0,
            budget_remaining: 1.0,
        }
    }
}

/// 爬取动作（DRL 模型输出）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlAction {
    /// URL 优先级调整因子（0.5-2.0，1.0 = 不变）
    pub url_priority_adjustment: f32,
    /// 并发度变化（-3 到 +3，0 = 不变）
    pub concurrency_delta: i32,
    /// 引擎选择偏好（0 = reqwest, 1 = playwright, 2 = tls_fingerprint, 3 = mllm）
    pub engine_selection: u8,
    /// 重试决策（0 = 不重试, 1 = 立即重试, 2 = 延迟重试）
    pub retry_decision: u8,
}

impl CrawlAction {
    /// 默认动作（不调整）
    pub fn noop() -> Self {
        Self {
            url_priority_adjustment: 1.0,
            concurrency_delta: 0,
            engine_selection: 0,
            retry_decision: 0,
        }
    }

    /// 保守动作（降低并发，提高优先级）
    pub fn conservative() -> Self {
        Self {
            url_priority_adjustment: 1.2,
            concurrency_delta: -1,
            engine_selection: 0,
            retry_decision: 2,
        }
    }

    /// 激进动作（增加并发，降低优先级阈值）
    pub fn aggressive() -> Self {
        Self {
            url_priority_adjustment: 0.8,
            concurrency_delta: 2,
            engine_selection: 0,
            retry_decision: 1,
        }
    }
}

impl Default for CrawlAction {
    fn default() -> Self {
        Self::noop()
    }
}

/// ONNX 推理 trait（抽象模型推理，便于测试和替换）
pub trait OnnxInference: Send + Sync {
    /// 运行推理：输入特征向量 → 输出动作
    fn predict(&self, features: &[f32]) -> Result<Vec<f32>, String>;
}

/// 启发式策略（无 ONNX 模型时的退化实现）
pub struct HeuristicPolicy;

impl OnnxInference for HeuristicPolicy {
    fn predict(&self, features: &[f32]) -> Result<Vec<f32>, String> {
        if features.len() < 5 {
            return Err(format!("expected 5 features, got {}", features.len()));
        }

        let _queue_depth = features[0];
        let response_time = features[1];
        let success_rate = features[2];
        let memory_pressure = features[3];
        let budget_remaining = features[4];

        // 启发式规则
        let priority_adj = if success_rate < 0.5 {
            1.5 // 成功率低 → 提高优先级阈值
        } else if success_rate > 0.9 {
            0.8 // 成功率高 → 降低阈值，加速爬取
        } else {
            1.0
        };

        let concurrency = if memory_pressure > 0.8 {
            -2 // 内存压力高 → 减少并发
        } else if success_rate > 0.8 && budget_remaining > 0.5 {
            1 // 状态好 → 增加并发
        } else {
            0
        };

        let engine = if response_time > 0.5 {
            1 // 响应慢 → 换用 playwright
        } else {
            0 // 正常 → reqwest
        };

        let retry = if success_rate < 0.3 {
            2 // 成功率很低 → 延迟重试
        } else if success_rate < 0.6 {
            1 // 成功率中等 → 立即重试
        } else {
            0 // 成功率高 → 不重试
        };

        Ok(vec![
            priority_adj,
            concurrency as f32,
            engine as f32,
            retry as f32,
        ])
    }
}

/// DRL 策略
///
/// 加载预训练 ONNX 模型，根据爬取状态预测最优动作。
/// 模型不可用时退化为启发式规则。
pub struct DrlPolicy {
    inference: Box<dyn OnnxInference>,
    /// 是否启用（默认 false）
    enabled: bool,
}

impl DrlPolicy {
    /// 使用指定推理后端创建策略
    pub fn new(inference: Box<dyn OnnxInference>, enabled: bool) -> Self {
        Self { inference, enabled }
    }

    /// 创建启发式策略（无 ONNX 模型）
    pub fn heuristic(enabled: bool) -> Self {
        Self {
            inference: Box::new(HeuristicPolicy),
            enabled,
        }
    }

    /// 预测最优动作
    pub fn predict(&self, state: &CrawlState) -> CrawlAction {
        if !self.enabled {
            return CrawlAction::noop();
        }

        // 验证状态
        if let Err(e) = state.validate() {
            log::warn!(
                "Invalid crawl state for DRL prediction: {}, using default",
                e
            );
            return CrawlAction::noop();
        }

        let features = state.to_feature_vector();
        match self.inference.predict(&features) {
            Ok(output) => self.decode_output(&output),
            Err(e) => {
                log::error!("DRL inference failed: {}, using default action", e);
                CrawlAction::noop()
            }
        }
    }

    /// 解码模型输出为动作
    fn decode_output(&self, output: &[f32]) -> CrawlAction {
        if output.len() < 4 {
            log::warn!("DRL output too short ({} < 4), using default", output.len());
            return CrawlAction::noop();
        }

        CrawlAction {
            url_priority_adjustment: output[0].clamp(0.5, 2.0),
            concurrency_delta: output[1].round().clamp(-3.0, 3.0) as i32,
            engine_selection: output[2].round().clamp(0.0, 3.0) as u8,
            retry_decision: output[3].round().clamp(0.0, 2.0) as u8,
        }
    }

    /// 是否启用
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Mock ONNX Inference ===

    struct MockInference {
        output: Vec<f32>,
    }

    impl OnnxInference for MockInference {
        fn predict(&self, _features: &[f32]) -> Result<Vec<f32>, String> {
            Ok(self.output.clone())
        }
    }

    struct FailingInference;

    impl OnnxInference for FailingInference {
        fn predict(&self, _features: &[f32]) -> Result<Vec<f32>, String> {
            Err("Mock inference failure".to_string())
        }
    }

    // === T085: 测试 ===

    #[test]
    fn test_crawl_state_to_feature_vector() {
        let state = CrawlState {
            queue_depth: 500,
            domain_response_time_avg: 2000.0,
            success_rate: 0.8,
            memory_pressure: 0.5,
            budget_remaining: 0.7,
        };

        let features = state.to_feature_vector();
        assert_eq!(features.len(), 5);
        assert!((features[0] - 0.5).abs() < 0.01); // 500/1000
        assert!((features[1] - 0.2).abs() < 0.01); // 2000/10000
    }

    #[test]
    fn test_crawl_state_validate_valid() {
        let state = CrawlState::default();
        assert!(state.validate().is_ok());
    }

    #[test]
    fn test_crawl_state_validate_invalid_success_rate() {
        let state = CrawlState {
            success_rate: 1.5,
            ..Default::default()
        };
        assert!(state.validate().is_err());
    }

    #[test]
    fn test_crawl_state_validate_invalid_memory_pressure() {
        let state = CrawlState {
            memory_pressure: -0.1,
            ..Default::default()
        };
        assert!(state.validate().is_err());
    }

    #[test]
    fn test_drl_predict_with_mock() {
        let mock = MockInference {
            output: vec![1.5, 2.0, 1.0, 1.0],
        };
        let policy = DrlPolicy::new(Box::new(mock), true);
        let state = CrawlState::default();

        let action = policy.predict(&state);
        assert_eq!(action.url_priority_adjustment, 1.5);
        assert_eq!(action.concurrency_delta, 2);
        assert_eq!(action.engine_selection, 1);
        assert_eq!(action.retry_decision, 1);
    }

    #[test]
    fn test_drl_predict_disabled_returns_noop() {
        let mock = MockInference {
            output: vec![1.5, 2.0, 1.0, 1.0],
        };
        let policy = DrlPolicy::new(Box::new(mock), false);
        let state = CrawlState::default();

        let action = policy.predict(&state);
        assert_eq!(action.url_priority_adjustment, 1.0);
        assert_eq!(action.concurrency_delta, 0);
    }

    #[test]
    fn test_drl_predict_invalid_state_returns_noop() {
        let mock = MockInference {
            output: vec![1.5, 2.0, 1.0, 1.0],
        };
        let policy = DrlPolicy::new(Box::new(mock), true);
        let state = CrawlState {
            success_rate: 2.0, // invalid
            ..Default::default()
        };

        let action = policy.predict(&state);
        assert_eq!(action.url_priority_adjustment, 1.0);
    }

    #[test]
    fn test_drl_predict_inference_failure_returns_noop() {
        let policy = DrlPolicy::new(Box::new(FailingInference), true);
        let state = CrawlState::default();

        let action = policy.predict(&state);
        assert_eq!(action.url_priority_adjustment, 1.0);
    }

    #[test]
    fn test_drl_predict_output_clamped() {
        let mock = MockInference {
            output: vec![5.0, 10.0, 255.0, 100.0], // 全部超范围
        };
        let policy = DrlPolicy::new(Box::new(mock), true);
        let state = CrawlState::default();

        let action = policy.predict(&state);
        assert_eq!(action.url_priority_adjustment, 2.0); // clamped to max
        assert_eq!(action.concurrency_delta, 3); // clamped to max
        assert_eq!(action.engine_selection, 3); // clamped to max
        assert_eq!(action.retry_decision, 2); // clamped to max
    }

    #[test]
    fn test_heuristic_policy_low_success_rate() {
        let policy = DrlPolicy::heuristic(true);
        let state = CrawlState {
            queue_depth: 100,
            domain_response_time_avg: 500.0,
            success_rate: 0.2, // 低于 0.3 → 延迟重试
            memory_pressure: 0.3,
            budget_remaining: 0.8,
        };

        let action = policy.predict(&state);
        assert!(
            action.url_priority_adjustment > 1.0,
            "low success rate should increase priority"
        );
        assert_eq!(
            action.retry_decision, 2,
            "very low success rate should use delayed retry"
        );
    }

    #[test]
    fn test_heuristic_policy_high_memory_pressure() {
        let policy = DrlPolicy::heuristic(true);
        let state = CrawlState {
            queue_depth: 100,
            domain_response_time_avg: 500.0,
            success_rate: 0.9,
            memory_pressure: 0.9,
            budget_remaining: 0.8,
        };

        let action = policy.predict(&state);
        assert!(
            action.concurrency_delta < 0,
            "high memory should reduce concurrency"
        );
    }

    #[test]
    fn test_crawl_action_noop() {
        let action = CrawlAction::noop();
        assert_eq!(action.url_priority_adjustment, 1.0);
        assert_eq!(action.concurrency_delta, 0);
        assert_eq!(action.engine_selection, 0);
        assert_eq!(action.retry_decision, 0);
    }

    #[test]
    fn test_drl_short_output_returns_noop() {
        let mock = MockInference {
            output: vec![1.0], // too short
        };
        let policy = DrlPolicy::new(Box::new(mock), true);
        let state = CrawlState::default();

        let action = policy.predict(&state);
        assert_eq!(
            action.url_priority_adjustment, 1.0,
            "short output should default to noop"
        );
    }
}
