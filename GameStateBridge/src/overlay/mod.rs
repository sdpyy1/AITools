pub mod config;
pub mod window;

use crate::core::manager::AdapterManager;
use crate::core::state::{GameType, MatchPhase, UnifiedGameState};
use config::{OverlayConfig, SharedOverlayConfig};
use serde_json::Value;
use std::sync::Arc;
use tokio::time::{Duration, interval};
use tracing::info;
use window::{
    OverlayDebugSnapshot, OverlayWindowState, SharedOverlayWindowState, UserEvent,
    run_overlay_thread, send_overlay_event,
};

/// Overlay 协调器
/// 负责监听 GSI 数据，控制 Overlay 窗口的显示/隐藏
#[derive(Clone)]
pub struct OverlayCoordinator {
    window_state: SharedOverlayWindowState,
    config: SharedOverlayConfig,
}

impl OverlayCoordinator {
    /// 创建并启动 Overlay 系统
    pub fn start(manager: Arc<AdapterManager>) -> Self {
        let config = config::create_shared_config();
        let initial_config = {
            let c = config.lock().unwrap();
            c.clone()
        };

        // 创建窗口状态
        let window_state = Arc::new(std::sync::Mutex::new(OverlayWindowState::new(
            initial_config,
        )));

        // 启动窗口线程
        run_overlay_thread(window_state.clone());
        info!("Overlay window thread started");

        let coordinator = Self {
            window_state: window_state.clone(),
            config: config.clone(),
        };

        // 启动 GSI 监听任务
        tokio::spawn(gsi_watcher(manager, window_state, config));

        coordinator
    }

    /// 获取当前配置（用于 Web API）
    pub fn get_config(&self) -> OverlayConfig {
        let ws = self.window_state.lock().unwrap();
        ws.config.clone()
    }

    /// 更新配置（用于 Web API）
    pub fn update_config(&self, new_config: OverlayConfig) {
        {
            let mut c = self.config.lock().unwrap();
            *c = new_config.clone();
            c.save();
        }

        {
            let mut ws = self.window_state.lock().unwrap();
            ws.config = new_config.clone();
        }

        send_overlay_event(&self.window_state, UserEvent::UpdateUrl(new_config.url));
    }

    /// 强制显示窗口（测试用）
    pub fn show(&self) {
        {
            let mut ws = self.window_state.lock().unwrap();
            ws.visible = true;
            ws.debug.visible = true;
        }
        send_overlay_event(&self.window_state, UserEvent::Show);
    }

    /// 强制隐藏窗口（测试用）
    pub fn hide(&self) {
        {
            let mut ws = self.window_state.lock().unwrap();
            ws.visible = false;
            ws.debug.visible = false;
        }
        send_overlay_event(&self.window_state, UserEvent::Hide);
    }

    /// 获取调试信息
    pub fn get_debug(&self) -> OverlayDebugSnapshot {
        let ws = self.window_state.lock().unwrap();
        ws.debug.clone()
    }

    /// 停止 Overlay 系统
    pub fn stop(&self) {
        send_overlay_event(&self.window_state, UserEvent::Exit);
    }
}

/// 后台任务：监听 GSI 中的玩家生死状态
async fn gsi_watcher(
    manager: Arc<AdapterManager>,
    window_state: SharedOverlayWindowState,
    config: SharedOverlayConfig,
) {
    let mut ticker = interval(Duration::from_millis(500));
    let mut last_alive: Option<bool> = None;

    loop {
        ticker.tick().await;

        // 读取当前配置
        let current_config = {
            let c = config.lock().unwrap();
            c.clone()
        };

        if !current_config.enabled {
            let mut ws = window_state.lock().unwrap();
            if ws.visible {
                ws.visible = false;
                ws.debug.visible = false;
                drop(ws);
                send_overlay_event(&window_state, UserEvent::Hide);
            }
            continue;
        }

        // 获取 CS2 状态
        let state = match manager.get_state(GameType::Cs2).await {
            Ok(s) => s,
            Err(_) => {
                let mut ws = window_state.lock().unwrap();
                if ws.visible {
                    ws.visible = false;
                    ws.debug.visible = false;
                    drop(ws);
                    send_overlay_event(&window_state, UserEvent::Hide);
                }
                last_alive = Some(true);
                continue;
            }
        };

        if state.match_phase == MatchPhase::Warmup {
            let mut ws = window_state.lock().unwrap();
            let was_visible = ws.visible;
            ws.visible = false;
            ws.debug = OverlayDebugSnapshot {
                player_name: current_config.player_name.clone(),
                player_steam_id: current_config.player_steam_id.clone(),
                detected_health: None,
                is_alive: true,
                source: "warmup_skip".to_string(),
                matched_name: None,
                matched_steam_id: None,
                spectating: false,
                visible: false,
            };
            if was_visible {
                info!("CS2 warmup phase, hiding death browser");
                drop(ws);
                send_overlay_event(&window_state, UserEvent::Hide);
            }
            last_alive = Some(true);
            continue;
        }

        let detection = detect_player_health(&state, &current_config);
        let my_health = detection.health;
        let keep_dead_while_spectating =
            my_health.is_none() && detection.spectating && last_alive == Some(false);
        let is_alive = match my_health {
            Some(health) => health > 0,
            None if keep_dead_while_spectating => false,
            None => true,
        };
        let debug_source = if keep_dead_while_spectating {
            format!("{}_keep_dead", detection.source)
        } else {
            detection.source.clone()
        };

        info!(
            "GSI health check: player_name={}, player_steam_id={:?}, health={:?}, source={}, alive={}, last_alive={:?}",
            current_config.player_name,
            current_config.player_steam_id,
            my_health,
            debug_source,
            is_alive,
            last_alive
        );

        let mut ws = window_state.lock().unwrap();
        ws.debug = OverlayDebugSnapshot {
            player_name: current_config.player_name.clone(),
            player_steam_id: current_config.player_steam_id.clone(),
            detected_health: my_health,
            is_alive,
            source: debug_source,
            matched_name: detection.matched_name.clone(),
            matched_steam_id: detection.matched_steam_id.clone(),
            spectating: detection.spectating,
            visible: ws.visible,
        };

        match (is_alive, last_alive) {
            // 死亡（从存活变为死亡）
            (false, Some(true)) | (false, None) => {
                if !ws.visible {
                    info!("Player died, showing overlay");
                    ws.visible = true;
                    ws.debug.visible = true;
                    drop(ws);
                    send_overlay_event(&window_state, UserEvent::Show);
                }
            }
            // 复活（从死亡变为存活）
            (true, Some(false)) => {
                if ws.visible {
                    info!("Player respawned, hiding overlay");
                    ws.visible = false;
                    ws.debug.visible = false;
                    drop(ws);
                    send_overlay_event(&window_state, UserEvent::Hide);
                }
            }
            // 状态未变化，不做任何事
            _ => {}
        }

        last_alive = Some(is_alive);
    }
}

#[derive(Debug, Clone)]
struct HealthDetection {
    health: Option<i32>,
    source: String,
    matched_name: Option<String>,
    matched_steam_id: Option<String>,
    spectating: bool,
}

fn detect_player_health(state: &UnifiedGameState, config: &OverlayConfig) -> HealthDetection {
    let raw = state.raw_payload.as_ref();
    let provider_steam_id = raw
        .and_then(|p| p.get("provider"))
        .and_then(|p| p.get("steamid"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let configured_steam_id = config
        .player_steam_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let identity_steam_id = configured_steam_id.or(provider_steam_id);
    let player_name = config.player_name.trim();

    let raw_player = raw.and_then(|p| p.get("player"));
    let raw_player_steam_id = raw_player.and_then(player_steam_id);
    let raw_activity = raw_player
        .and_then(|p| p.get("activity"))
        .and_then(|v| v.as_str());
    let raw_spectarget = raw_player
        .and_then(|p| p.get("spectarget"))
        .and_then(|v| v.as_str());
    let observing_other_player = identity_steam_id
        .as_deref()
        .zip(raw_player_steam_id)
        .map(|(identity, observed)| identity != observed)
        .unwrap_or(false);
    let spectating =
        raw_spectarget.is_some() || raw_activity == Some("spectating") || observing_other_player;

    if let Some(identity_steam_id) = identity_steam_id.as_deref() {
        if let Some(player) = find_allplayer_by_steam_id(raw, identity_steam_id) {
            return detection_from_player(
                player,
                "allplayers_by_steamid",
                false,
                Some(identity_steam_id.to_string()),
            );
        }

        if raw_player_steam_id == Some(identity_steam_id) {
            return detection_from_player(
                raw_player.unwrap(),
                "raw_player_matching_steamid",
                spectating,
                Some(identity_steam_id.to_string()),
            );
        }

        if observing_other_player {
            return HealthDetection {
                health: Some(0),
                source: "raw_player_other_steamid_assume_dead".to_string(),
                matched_name: Some(config.player_name.clone()),
                matched_steam_id: Some(identity_steam_id.to_string()),
                spectating: true,
            };
        }
    }

    if !player_name.is_empty() {
        if let Some(player) = find_allplayer_by_name(raw, player_name) {
            return detection_from_player(player, "allplayers_by_name", false, None);
        }

        if !spectating && raw_player.and_then(player_name_from_value) == Some(player_name) {
            return detection_from_player(
                raw_player.unwrap(),
                "raw_player_matching_name",
                false,
                None,
            );
        }
    }

    HealthDetection {
        health: None,
        source: if spectating {
            "no_identity_match_spectating".to_string()
        } else {
            "no_identity_match".to_string()
        },
        matched_name: None,
        matched_steam_id: identity_steam_id,
        spectating,
    }
}

fn find_allplayer_by_steam_id<'a>(raw: Option<&'a Value>, steam_id: &str) -> Option<&'a Value> {
    raw.and_then(|p| p.get("allplayers"))
        .and_then(|ap| ap.as_object())
        .and_then(|players| {
            players.iter().find_map(|(key, player)| {
                let value_steam_id = player_steam_id(player).unwrap_or(key.as_str());
                if value_steam_id == steam_id {
                    Some(player)
                } else {
                    None
                }
            })
        })
}

fn find_allplayer_by_name<'a>(raw: Option<&'a Value>, player_name: &str) -> Option<&'a Value> {
    raw.and_then(|p| p.get("allplayers"))
        .and_then(|ap| ap.as_object())
        .and_then(|players| {
            players
                .values()
                .find(|player| player_name_from_value(player) == Some(player_name))
        })
}

fn detection_from_player(
    player: &Value,
    source: &str,
    spectating: bool,
    fallback_steam_id: Option<String>,
) -> HealthDetection {
    HealthDetection {
        health: player_health(player),
        source: source.to_string(),
        matched_name: player_name_from_value(player).map(str::to_string),
        matched_steam_id: player_steam_id(player)
            .map(str::to_string)
            .or(fallback_steam_id),
        spectating,
    }
}

fn player_health(player: &Value) -> Option<i32> {
    player
        .get("state")
        .and_then(|s| s.get("health"))
        .and_then(|h| h.as_i64())
        .map(|h| h as i32)
}

fn player_name_from_value(player: &Value) -> Option<&str> {
    player.get("name").and_then(|n| n.as_str())
}

fn player_steam_id(player: &Value) -> Option<&str> {
    player.get("steamid").and_then(|id| id.as_str())
}
