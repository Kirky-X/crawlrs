// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

/// 每队并发信号量管理器
///
/// 为每个团队提供一个独立的并发信号量，以限制其并发请求数。
#[derive(Clone, Debug)]
pub struct TeamSemaphore {
    /// 存储每队的信号量
    semaphores: Arc<DashMap<Uuid, Arc<Semaphore>>>,
    /// 默认并发数
    default_permits: usize,
}

impl TeamSemaphore {
    /// 创建一个新的TeamSemaphore实例
    ///
    /// # 参数
    ///
    /// * `default_permits` - 每个团队的默认并发许可数
    ///
    /// # 返回值
    ///
    /// 返回新的TeamSemaphore实例
    pub fn new(default_permits: usize) -> Self {
        Self {
            semaphores: Arc::new(DashMap::new()),
            default_permits,
        }
    }

    /// 获取指定团队的信号量许可
    ///
    /// 如果该团队的信号量不存在，则会创建一个新的。
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    ///
    /// # 返回值
    ///
    /// 返回一个信号量许可
    pub async fn acquire(&self, team_id: Uuid) -> OwnedSemaphorePermit {
        self.get_or_create(team_id).acquire_owned().await.unwrap()
    }

    /// 获取或创建指定团队的信号量
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    ///
    /// # 返回值
    ///
    /// 返回一个Arc包装的信号量
    fn get_or_create(&self, team_id: Uuid) -> Arc<Semaphore> {
        self.semaphores
            .entry(team_id)
            .or_insert_with(|| Arc::new(Semaphore::new(self.default_permits)))
            .clone()
    }
}
