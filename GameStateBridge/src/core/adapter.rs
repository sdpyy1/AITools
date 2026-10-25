use super::state::{AdapterStatus, GameType, UnifiedGameState};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// 游戏适配器 Trait
/// 每个游戏需实现此接口，供上层统一调用
/// 所有方法均为 &self，内部使用 RwLock/Mutex 处理可变状态
#[async_trait]
pub trait GameAdapter: Send + Sync {
    /// 返回适配器对应的游戏类型
    fn game_type(&self) -> GameType;

    /// 获取当前最新对局状态
    async fn current_state(&self) -> Result<UnifiedGameState>;

    /// 获取适配器状态摘要
    fn status(&self) -> AdapterStatus;

    /// 执行一个动作指令（默认不支持）。返回 JSON 结果。
    async fn command(&self, _cmd: &str, _payload: Value) -> Result<Value> {
        anyhow::bail!("该适配器不支持指令")
    }
}
