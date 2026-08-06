#!/usr/bin/env python3
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in project root for full license information.

"""DRL 爬取策略训练脚本

定义 MDP 环境（gymnasium）、DQN agent（stable-baselines3）、
奖励函数、训练循环、导出 ONNX。

## 用法

```bash
pip install gymnasium stable-baselines3 torch onnx
python scripts/drl/train_policy.py --episodes 10000 --output models/crawl_policy.onnx
```

## 状态空间（5 维）

- queue_depth: 队列深度（归一化到 0-1）
- domain_response_time_avg: 平均响应时间（归一化到 0-1）
- success_rate: 成功率（0-1）
- memory_pressure: 内存压力（0-1）
- budget_remaining: 剩余预算（0-1）

## 动作空间（4 维，离散化）

- url_priority_adjustment: [0.5, 1.0, 1.5, 2.0]
- concurrency_delta: [-2, -1, 0, 1, 2]
- engine_selection: [0=reqwest, 1=playwright, 2=tls, 3=mllm]
- retry_decision: [0=none, 1=immediate, 2=delayed]
"""

import argparse
import os
import sys

import numpy as np

try:
    import gymnasium as gym
    from gymnasium import spaces
except ImportError:
    print("ERROR: gymnasium not installed. Run: pip install gymnasium")
    sys.exit(1)


class CrawlEnv(gym.Env):
    """爬取环境 MDP

    模拟爬取过程中的状态转换，奖励函数设计：
    - 成功提取: +R
    - 被拦截: -R
    - 超时: -R/2
    - 覆盖率提升: +bonus
    """

    metadata = {"render_modes": []}

    def __init__(self, max_steps: int = 200):
        super().__init__()
        self.max_steps = max_steps
        self.current_step = 0

        # 状态空间: 5 维连续 [0, 1]
        self.observation_space = spaces.Box(
            low=0.0, high=1.0, shape=(5,), dtype=np.float32
        )

        # 动作空间: 4 维离散
        # 0-3: priority_adj, 0-4: concurrency, 0-3: engine, 0-2: retry
        self.action_space = spaces.MultiDiscrete([4, 5, 4, 3])

        self.state = None

    def reset(self, seed=None, options=None):
        super().reset(seed=seed)
        self.current_step = 0
        self.state = np.array(
            [0.5, 0.3, 0.8, 0.2, 1.0], dtype=np.float32
        )  # 初始状态
        return self.state, {}

    def step(self, action):
        self.current_step += 1

        # 解码动作
        priority_idx, concurrency_idx, engine_idx, retry_idx = action

        # 模拟状态转换（简化模型）
        noise = self.np_random.normal(0, 0.05, size=5).astype(np.float32)

        # 动作影响
        concurrency_effect = (concurrency_idx - 2) * 0.05  # -0.1 to +0.1
        success_effect = (3 - engine_idx) * 0.02  # 好引擎提升成功率

        self.state = np.clip(
            self.state
            + noise
            + np.array([0, 0, success_effect, -0.02, -0.005], dtype=np.float32),
            0.0,
            1.0,
        )

        # 奖励计算
        reward = 0.0
        success_rate = self.state[2]

        # 成功提取奖励
        if success_rate > 0.7:
            reward += 1.0
        elif success_rate > 0.5:
            reward += 0.5
        else:
            reward -= 0.5  # 被拦截

        # 内存压力惩罚
        if self.state[3] > 0.8:
            reward -= 1.0

        # 预算效率奖励
        if self.state[4] > 0.3:
            reward += 0.2

        # 终止条件
        terminated = self.state[4] <= 0.0 or success_rate < 0.1
        truncated = self.current_step >= self.max_steps

        return self.state, reward, terminated, truncated, {}


def train(args):
    """训练 DQN agent"""
    try:
        from stable_baselines3 import DQN
    except ImportError:
        print("ERROR: stable-baselines3 not installed. Run: pip install stable-baselines3")
        sys.exit(1)

    env = CrawlEnv(max_steps=args.episodes // 10)

    print(f"Training DQN agent for {args.episodes} timesteps...")
    model = DQN(
        "MlpPolicy",
        env,
        learning_rate=0.001,
        buffer_size=10000,
        batch_size=64,
        gamma=0.95,
        verbose=1,
    )

    model.learn(total_timesteps=args.episodes)

    # 导出 ONNX
    if args.output:
        export_onnx(model, args.output)
        print(f"Model exported to {args.output}")


def export_onnx(model, output_path):
    """导出模型为 ONNX 格式"""
    try:
        import torch
    except ImportError:
        print("ERROR: torch not installed. Run: pip install torch")
        sys.exit(1)

    os.makedirs(os.path.dirname(output_path) or ".", exist_ok=True)

    # 创建 dummy 输入
    dummy_input = torch.randn(1, 5, dtype=torch.float32)

    # 提取策略网络
    class PolicyWrapper(torch.nn.Module):
        def __init__(self, policy):
            super().__init__()
            self.policy = policy

        def forward(self, x):
            # 简化的前向传播
            features, latent_pi = self.policy.q_net.features_extractor(x), None
            q_values = self.policy.q_net.q_net(features)
            return q_values

    wrapper = PolicyWrapper(model.policy)
    wrapper.eval()

    torch.onnx.export(
        wrapper,
        dummy_input,
        output_path,
        input_names=["state"],
        output_names=["q_values"],
        dynamic_axes={"state": {0: "batch_size"}, "q_values": {0: "batch_size"}},
        opset_version=13,
    )
    print(f"ONNX model saved to {output_path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Train DRL crawl policy")
    parser.add_argument(
        "--episodes", type=int, default=10000, help="Training timesteps"
    )
    parser.add_argument(
        "--output", type=str, default="models/crawl_policy.onnx", help="Output ONNX path"
    )
    args = parser.parse_args()

    train(args)
