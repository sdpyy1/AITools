//! LOL 适配器配置（可持久化到 lol_config.json）

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LolConfig {
    /// 队列名称子串，用于在 /lol-game-queues 中匹配目标模式（如 "海克斯大乱斗" / "ARAM"）
    #[serde(default = "default_mode_name")]
    pub mode_name: String,

    /// 显式指定 queueId（优先于 mode_name）
    #[serde(default)]
    pub queue_id: Option<u32>,

    /// 阵亡时打开的网址（抖音网页版）
    #[serde(default = "default_douyin")]
    pub douyin_url: String,

    /// 是否启用死亡自动切换抖音
    #[serde(default = "default_true")]
    pub death_switch_enabled: bool,

    /// 复活前提前多少秒切回游戏
    #[serde(default = "default_respawn")]
    pub respawn_warning_seconds: f64,

    /// 死亡检测轮询间隔（秒）
    #[serde(default = "default_poll")]
    pub poll_interval_seconds: f64,

    /// 上等马评选结果是否发到游戏内聊天
    #[serde(default = "default_true")]
    pub chat_enabled: bool,

    /// 是否启用自动接受对局（ready-check 出现即自动接受）
    #[serde(default = "default_true")]
    pub auto_accept_enabled: bool,
}

fn default_mode_name() -> String {
    "海克斯大乱斗".into()
}
fn default_douyin() -> String {
    "https://www.douyin.com/".into()
}
fn default_true() -> bool {
    true
}
fn default_respawn() -> f64 {
    5.0
}
fn default_poll() -> f64 {
    1.0
}

impl Default for LolConfig {
    fn default() -> Self {
        Self {
            mode_name: default_mode_name(),
            queue_id: None,
            douyin_url: default_douyin(),
            death_switch_enabled: true,
            respawn_warning_seconds: 5.0,
            poll_interval_seconds: 1.0,
            chat_enabled: true,
            auto_accept_enabled: true,
        }
    }
}

impl LolConfig {
    pub fn config_path() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join("lol_config.json");
            }
        }
        PathBuf::from("lol_config.json")
    }

    pub fn load_or_default() -> Self {
        let p = Self::config_path();
        match std::fs::read_to_string(&p) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let p = Self::config_path();
        std::fs::write(&p, serde_json::to_string_pretty(self).unwrap())
    }
}

pub type SharedLolConfig = Arc<RwLock<LolConfig>>;
