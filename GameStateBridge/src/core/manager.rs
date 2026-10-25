use super::adapter::GameAdapter;
use super::registry::AdapterRegistry;
use super::state::{AdapterStatus, GameType, UnifiedGameState};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 适配器管理器
/// 负责管理当前活跃的游戏适配器，支持动态切换游戏
pub struct AdapterManager {
    registry: AdapterRegistry,
    /// 当前活跃的适配器
    active_adapters: RwLock<HashMap<GameType, Arc<dyn GameAdapter>>>,
}

impl AdapterManager {
    pub fn new(registry: AdapterRegistry) -> Self {
        Self {
            registry,
            active_adapters: RwLock::new(HashMap::new()),
        }
    }

    /// 激活指定游戏的适配器（由工厂函数完成初始化）
    pub async fn activate<F>(&self, game_type: GameType, factory: F) -> anyhow::Result<()>
    where
        F: FnOnce() -> anyhow::Result<Arc<dyn GameAdapter>>,
    {
        if !self.registry.is_supported(game_type) {
            anyhow::bail!("Game {:?} is not supported", game_type);
        }

        // 如果已经激活，先移除（Drop 会自动清理资源）
        self.deactivate(game_type).await;

        let adapter = factory()?;

        let mut adapters = self.active_adapters.write().await;
        adapters.insert(game_type, adapter);

        Ok(())
    }

    /// 关闭指定游戏的适配器
    pub async fn deactivate(&self, game_type: GameType) {
        let mut adapters = self.active_adapters.write().await;
        adapters.remove(&game_type);
        // Arc 释放后，如果实现 Drop，会自动清理资源
    }

    /// 获取指定游戏的当前状态
    pub async fn get_state(&self, game_type: GameType) -> anyhow::Result<UnifiedGameState> {
        let adapters = self.active_adapters.read().await;
        let adapter = adapters
            .get(&game_type)
            .ok_or_else(|| anyhow::anyhow!("Adapter for {:?} is not active", game_type))?;

        adapter.current_state().await
    }

    /// 获取指定游戏适配器的状态
    pub async fn get_status(&self, game_type: GameType) -> Option<AdapterStatus> {
        let adapters = self.active_adapters.read().await;
        adapters.get(&game_type).map(|a| a.status())
    }

    /// 获取所有活跃适配器的状态
    pub async fn all_statuses(&self) -> Vec<AdapterStatus> {
        let adapters = self.active_adapters.read().await;
        adapters.values().map(|a| a.status()).collect()
    }

    /// 获取所有支持的游戏类型
    pub fn supported_games(&self) -> Vec<GameType> {
        self.registry.available_games()
    }

    /// 对指定游戏的适配器执行指令
    pub async fn command(
        &self,
        game_type: GameType,
        cmd: &str,
        payload: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let adapters = self.active_adapters.read().await;
        let adapter = adapters
            .get(&game_type)
            .ok_or_else(|| anyhow::anyhow!("Adapter for {:?} is not active", game_type))?;
        adapter.command(cmd, payload).await
    }

    /// 检查某游戏是否已激活
    pub async fn is_active(&self, game_type: GameType) -> bool {
        let adapters = self.active_adapters.read().await;
        adapters.contains_key(&game_type)
    }
}
