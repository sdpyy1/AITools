use super::adapter::GameAdapter;
use super::state::GameType;
use std::collections::HashMap;
use std::sync::Arc;

/// 游戏适配器注册表
/// 负责管理所有可用适配器的创建和生命周期
pub struct AdapterRegistry {
    factories: HashMap<GameType, Box<dyn Fn() -> Arc<dyn GameAdapter> + Send + Sync>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// 注册一个适配器工厂
    pub fn register<F>(&mut self, game_type: GameType, factory: F)
    where
        F: Fn() -> Arc<dyn GameAdapter> + Send + Sync + 'static,
    {
        self.factories.insert(game_type, Box::new(factory));
    }

    /// 创建指定游戏的适配器实例
    pub fn create(&self, game_type: GameType) -> Option<Arc<dyn GameAdapter>> {
        self.factories.get(&game_type).map(|f| f())
    }

    /// 获取所有已注册的游戏类型
    pub fn available_games(&self) -> Vec<GameType> {
        self.factories.keys().cloned().collect()
    }

    /// 检查某游戏是否已注册适配器
    pub fn is_supported(&self, game_type: GameType) -> bool {
        self.factories.contains_key(&game_type)
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}
