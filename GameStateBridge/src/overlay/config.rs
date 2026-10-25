use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Overlay 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    /// 总开关
    pub enabled: bool,
    /// 死亡时打开的网页 URL
    pub url: String,
    /// 玩家 SteamID64（优先用于在 GSI allplayers 中精确匹配自己，避免观战目标误判）
    pub player_steam_id: Option<String>,
    /// 玩家游戏内用户名（SteamID 未配置时的 fallback）
    pub player_name: String,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            url: "https://www.douyin.com".to_string(),
            player_steam_id: None,
            player_name: "爱喝东方树叶".to_string(),
        }
    }
}

impl OverlayConfig {
    /// 配置文件路径（放在项目目录下）
    pub fn config_path() -> PathBuf {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        exe_dir.join("overlay_config.json")
    }

    /// 从文件加载配置
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
        let default = Self::default();
        default.save();
        default
    }

    /// 保存配置到文件
    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, content);
        }
    }
}

/// 线程安全的配置包装
pub type SharedOverlayConfig = Arc<Mutex<OverlayConfig>>;

/// 创建共享配置
pub fn create_shared_config() -> SharedOverlayConfig {
    Arc::new(Mutex::new(OverlayConfig::load()))
}
