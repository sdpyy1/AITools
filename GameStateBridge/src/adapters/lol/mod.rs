pub mod commands;
pub mod config;
pub mod lcu;
pub mod live;
#[cfg(windows)]
pub mod window;

use crate::core::adapter::GameAdapter;
use crate::core::state::{AdapterStatus, GameType, MatchPhase, Team, UnifiedGameState, Player, PlayerStats};
use async_trait::async_trait;
use config::{LolConfig, SharedLolConfig};
use lcu::LcuClient;
use live::LiveClient;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// 进程级共享配置（适配器与死亡监视器共用，set_config 可实时更新）
static LOL_CONFIG: OnceLock<SharedLolConfig> = OnceLock::new();
static WATCHER_STARTED: AtomicBool = AtomicBool::new(false);
static ACCEPT_WATCHER_STARTED: AtomicBool = AtomicBool::new(false);

/// 战绩缓存有效期：对局内/英雄选择阶段玩家固定，没必要每秒都去拉 30+ 个请求
const ENRICH_CACHE_TTL: Duration = Duration::from_secs(60);

/// LOL 适配器：整合 LCU（控制/读写）与 Live API（死亡检测）
pub struct LolAdapter {
    config: SharedLolConfig,
    lcu: Mutex<Option<LcuClient>>,
    connected: RwLock<bool>,
    last_error: RwLock<Option<String>>,
    /// 玩家战绩缓存：签名(含队伍与 puuid) -> (数据, 时间戳)，TTL 内复用
    enrich_cache: Mutex<Option<(String, HashMap<String, commands::PlayerInfo>, Instant)>>,
}

impl LolAdapter {
    pub fn new() -> Self {
        let config = LOL_CONFIG
            .get_or_init(|| Arc::new(RwLock::new(LolConfig::load_or_default())))
            .clone();
        // 全局仅启动一个死亡监视器
        if WATCHER_STARTED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let cfg = config.clone();
            tokio::spawn(Self::death_watcher(cfg));
        }
        // 全局仅启动一个自动接受对局监视器
        if ACCEPT_WATCHER_STARTED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let cfg = config.clone();
            tokio::spawn(Self::ready_check_watcher(cfg));
        }
        Self {
            config,
            lcu: Mutex::new(None),
            connected: RwLock::new(false),
            last_error: RwLock::new(None),
            enrich_cache: Mutex::new(None),
        }
    }

    /// 取缓存的 LCU 客户端；无则发现并缓存
    async fn lcu(&self) -> anyhow::Result<LcuClient> {
        if let Some(c) = self.lcu.lock().await.clone() {
            return Ok(c);
        }
        self.rediscover().await
    }

    async fn rediscover(&self) -> anyhow::Result<LcuClient> {
        let creds = lcu::discover_credentials()?;
        let client = LcuClient::with_credentials(creds)?;
        *self.lcu.lock().await = Some(client.clone());
        *self.connected.write().unwrap() = true;
        Ok(client)
    }

    /// 死亡自动切换抖音的后台任务（由 main 启动一个全局实例）
    pub async fn death_watcher(config: SharedLolConfig) {
        #[cfg(windows)]
        window::disable_foreground_lock();
        let live = match LiveClient::new() {
            Ok(l) => l,
            Err(e) => {
                warn!("Live API 客户端初始化失败: {}", e);
                return;
            }
        };
        let mut prev_dead = false;
        loop {
            let cfg = config.read().unwrap().clone();
            if cfg.death_switch_enabled {
                match live.local_player().await {
                    Ok(p) => {
                        let dead = p
                            .get("isDead")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false);
                        if dead && !prev_dead {
                            info!("检测到阵亡，切换到抖音");
                            #[cfg(windows)]
                            window::open_or_focus_douyin(&cfg.douyin_url);
                            #[cfg(not(windows))]
                            let _ = webbrowser::open(&cfg.douyin_url);
                        } else if !dead && prev_dead {
                            info!("已复活，切回游戏");
                            #[cfg(windows)]
                            {
                                // 先聚焦抖音窗口并按空格暂停视频，再切回游戏，
                                // 全程不用全局媒体键，避免误触发 QQ 音乐等后台播放器
                                window::pause_douyin_via_space();
                                window::bring_lol_to_front();
                            }
                        }
                        prev_dead = dead;
                    }
                    Err(_) => {
                        prev_dead = false;
                    }
                }
            }
            let interval = config.read().unwrap().poll_interval_seconds.max(0.2);
            tokio::time::sleep(std::time::Duration::from_secs_f64(interval)).await;
        }
    }

    /// 自动接受对局的后台任务：轮询 ready-check，出现即接受。
    /// 独立于前端轮询，关掉页面也能接受。
    pub async fn ready_check_watcher(config: SharedLolConfig) {
        let mut client: Option<LcuClient> = None;
        loop {
            let cfg = config.read().unwrap().clone();
            if cfg.auto_accept_enabled {
                // 凭据失效（客户端重启/退出）时丢弃缓存，下次重新发现
                if client.is_none() {
                    if let Ok(creds) = lcu::discover_credentials() {
                        if let Ok(c) = LcuClient::with_credentials(creds) {
                            client = Some(c);
                        }
                    }
                }
                if let Some(c) = client.as_ref() {
                    match c.ready_check().await {
                        Ok(rc) => {
                            let state = rc
                                .get("state")
                                .and_then(|x| x.as_str())
                                .unwrap_or("");
                            if state == "InProgress" {
                                match c.accept_ready_check().await {
                                    Ok(_) => info!("已自动接受对局"),
                                    Err(e) => warn!("自动接受对局失败: {}", e),
                                }
                            }
                        }
                        Err(_) => {
                            client = None;
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}

#[async_trait]
impl GameAdapter for LolAdapter {
    fn game_type(&self) -> GameType {
        GameType::Lol
    }

    async fn current_state(&self) -> anyhow::Result<UnifiedGameState> {
        let mut state = UnifiedGameState::new(GameType::Lol);
        let client = match self.lcu().await {
            Ok(c) => c,
            Err(e) => {
                *self.last_error.write().unwrap() = Some(e.to_string());
                state.match_phase = MatchPhase::Menu;
                return Ok(state);
            }
        };
        *self.last_error.write().unwrap() = None;
        let phase = match client.gameflow_phase().await {
            Ok(p) => p,
            Err(e) => {
                // 客户端可能刚重启、端口变了：作废缓存，下次重新发现
                *self.lcu.lock().await = None;
                *self.connected.write().unwrap() = false;
                *self.last_error.write().unwrap() = Some(e.to_string());
                state.match_phase = MatchPhase::Menu;
                return Ok(state);
            }
        };
        state.match_phase = match phase.as_str() {
            "ChampSelect" => MatchPhase::Loading,
            "InProgress" => MatchPhase::Live,
            "GameStart" | "WaitingForStats" => MatchPhase::Intermission,
            "EndOfGame" | "PreEndOfGame" => MatchPhase::GameOver,
            "Matchmaking" | "ReadyCheck" => MatchPhase::Loading,
            _ => MatchPhase::Menu,
        };
        state.is_in_match = matches!(phase.as_str(), "InProgress" | "ChampSelect");

        if phase == "ChampSelect" {
            if let Ok(sess) = client.champ_select_session().await {
                let mut entries: Vec<(Team, String)> = Vec::new();
                for (team, t) in [("myTeam", Team::Blue), ("theirTeam", Team::Red)] {
                    if let Some(arr) = sess.get(team).and_then(|x| x.as_array()) {
                        for m in arr {
                            if let Some(pu) = m.get("puuid").and_then(|x| x.as_str()) {
                                if !pu.is_empty() {
                                    entries.push((t, pu.to_string()));
                                }
                            }
                        }
                    }
                }
                state.players = self.build_enriched_players(&client, entries).await;
            }
        } else if phase == "InProgress" {
            // 参考 lcu-driver 脚本：对局内也拉双方战绩
            if let Ok(sess) = client.gameflow_session().await {
                let local = client
                    .summoner_current()
                    .await
                    .ok()
                    .and_then(|v| v.get("puuid").and_then(|x| x.as_str()).map(|s| s.to_string()));
                let t1 = sess
                    .get("gameData")
                    .and_then(|g| g.get("teamOne"))
                    .and_then(|x| x.as_array())
                    .cloned()
                    .unwrap_or_default();
                let t2 = sess
                    .get("gameData")
                    .and_then(|g| g.get("teamTwo"))
                    .and_then(|x| x.as_array())
                    .cloned()
                    .unwrap_or_default();
                let local_in_t1 = local
                    .as_deref()
                    .map(|l| {
                        t1.iter()
                            .any(|p| p.get("puuid").and_then(|x| x.as_str()) == Some(l))
                    })
                    .unwrap_or(false);
                let mut entries: Vec<(Team, String)> = Vec::new();
                for p in t1.iter().chain(t2.iter()) {
                    if let Some(pu) = p.get("puuid").and_then(|x| x.as_str()) {
                        if !pu.is_empty() {
                            let on_t1 = t1.iter().any(|q| q.get("puuid").and_then(|x| x.as_str()) == Some(pu));
                            let t = if on_t1 {
                                if local_in_t1 { Team::Blue } else { Team::Red }
                            } else if local_in_t1 {
                                Team::Red
                            } else {
                                Team::Blue
                            };
                            entries.push((t, pu.to_string()));
                        }
                    }
                }
                state.players = self.build_enriched_players(&client, entries).await;
            }
        }
        state.raw_payload = Some(serde_json::json!({ "gameflow_phase": phase }));
        Ok(state)
    }

    fn status(&self) -> AdapterStatus {
        let connected = *self.connected.read().unwrap();
        let last_error = self.last_error.read().unwrap().clone();
        AdapterStatus {
            game_type: GameType::Lol,
            is_initialized: true,
            is_connected: connected,
            last_update: None,
            error_message: if connected {
                None
            } else {
                Some(
                    last_error
                        .unwrap_or_else(|| "LCU 未连接（客户端未运行或未登录）".into()),
                )
            },
        }
    }

    async fn command(&self, cmd: &str, payload: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        match cmd {
            "start_match" | "queue" => {
                let client = self.lcu().await?;
                let (mode_name, qid) = {
                    let c = self.config.read().unwrap();
                    (c.mode_name.clone(), c.queue_id)
                };
                let q = commands::resolve_queue_id(&client, &mode_name, qid).await?;
                // 若已在大厅（如上次残留），先退出再重建，避免 INVALID_LOBBY
                if client.get_lobby().await.is_ok() {
                    let _ = client.leave_lobby().await;
                }
                client.create_lobby(q).await?;
                client.start_matchmaking().await?;
                Ok(serde_json::json!({ "success": true, "queue_id": q }))
            }
            "stop_match" => {
                let client = self.lcu().await?;
                client.cancel_matchmaking().await?;
                Ok(serde_json::json!({ "success": true }))
            }
            "accept_match" => {
                let client = self.lcu().await?;
                client.accept_ready_check().await?;
                Ok(serde_json::json!({ "success": true }))
            }
            "list_queues" => {
                let client = self.lcu().await?;
                let queues = client.game_queues().await?;
                Ok(queues)
            }
            "evaluate" | "rank" => {
                let client = self.lcu().await?;
                let eval = commands::evaluate(&client).await?;
                let chat_enabled = self.config.read().unwrap().chat_enabled;
                let mut sent = false;
                if chat_enabled {
                    match commands::send_all_chat(&client, &eval.message).await {
                        Ok(()) => sent = true,
                        Err(e) => warn!("发送聊天失败: {}", e),
                    }
                }
                Ok(serde_json::json!({
                    "success": true,
                    "sent": sent,
                    "message": eval.message,
                    "players": eval.players,
                }))
            }
            "send_chat" => {
                let msg = payload
                    .get("message")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| anyhow::anyhow!("缺少 message 字段"))?;
                let client = self.lcu().await?;
                commands::send_all_chat(&client, msg).await?;
                Ok(serde_json::json!({ "success": true }))
            }
            "get_config" => {
                let c = self.config.read().unwrap();
                Ok(serde_json::to_value(&*c).unwrap())
            }
            "live_status" => {
                match LiveClient::new() {
                    Ok(live) => match commands::live_status(&live).await {
                        Ok(v) => Ok(v),
                        Err(_) => Ok(serde_json::json!({ "in_game": false })),
                    },
                    Err(_) => Ok(serde_json::json!({ "in_game": false })),
                }
            }
            "set_config" => {
                let mut c = self.config.write().unwrap();
                if let Some(v) = payload.get("mode_name").and_then(|x| x.as_str()) {
                    c.mode_name = v.to_string();
                }
                // 显式传 queue_id 数字则设置；传 null 则清空（回落到按 mode_name 解析）
                if let Some(v) = payload.get("queue_id") {
                    if v.is_null() {
                        c.queue_id = None;
                    } else if let Some(n) = v.as_u64() {
                        c.queue_id = Some(n as u32);
                    }
                }
                if let Some(v) = payload.get("douyin_url").and_then(|x| x.as_str()) {
                    c.douyin_url = v.to_string();
                }
                if let Some(v) = payload.get("death_switch_enabled").and_then(|x| x.as_bool()) {
                    c.death_switch_enabled = v;
                }
                if let Some(v) = payload.get("chat_enabled").and_then(|x| x.as_bool()) {
                    c.chat_enabled = v;
                }
                if let Some(v) = payload.get("auto_accept_enabled").and_then(|x| x.as_bool()) {
                    c.auto_accept_enabled = v;
                }
                if let Some(v) = payload.get("poll_interval_seconds").and_then(|x| x.as_f64()) {
                    c.poll_interval_seconds = v;
                }
                if let Some(v) = payload.get("respawn_warning_seconds").and_then(|x| x.as_f64()) {
                    c.respawn_warning_seconds = v;
                }
                let _ = c.save();
                Ok(serde_json::json!({ "success": true, "config": &*c }))
            }
            _ => anyhow::bail!("未知命令: {}", cmd),
        }
    }
}

/// 固有实现（非 trait 方法）：战绩拉取与缓存
impl LolAdapter {
    /// 根据 (队伍, puuid) 列表，并行拉取每个玩家的战绩（名字/段位/近期 KDA），
    /// 按 puuid 集合+队伍签名缓存 60 秒，避免每秒轮询都发 30+ 个请求。
    async fn build_enriched_players(
        &self,
        client: &LcuClient,
        entries: Vec<(Team, String)>,
    ) -> Vec<Player> {
        let sig = entries
            .iter()
            .map(|(t, p)| format!("{:?}{}", t, p))
            .collect::<Vec<_>>()
            .join(",");
        // 命中缓存且未过期
        {
            let cache = self.enrich_cache.lock().await;
            if let Some((csig, map, ts)) = cache.as_ref() {
                if csig == &sig && ts.elapsed() < ENRICH_CACHE_TTL {
                    return Self::entries_to_players(&entries, map);
                }
            }
        }
        let puuids: Vec<String> = entries.iter().map(|(_, p)| p.clone()).collect();
        let map = commands::enrich_players_map(client, &puuids).await;
        {
            let mut cache = self.enrich_cache.lock().await;
            *cache = Some((sig, map.clone(), Instant::now()));
        }
        Self::entries_to_players(&entries, &map)
    }

    fn entries_to_players(
        entries: &[(Team, String)],
        map: &HashMap<String, commands::PlayerInfo>,
    ) -> Vec<Player> {
        let mut players = Vec::new();
        for (t, pu) in entries {
            let mut stats = PlayerStats::default();
            if let Some(info) = map.get(pu) {
                stats.extra.insert("tier".into(), serde_json::json!(info.tier_label));
                stats.extra.insert("perf".into(), serde_json::json!(info.perf_label));
                stats.extra.insert("recent".into(), serde_json::json!(info.recent_detail));
                stats.extra.insert("score".into(), serde_json::json!(info.score));
                stats.extra.insert("level".into(), serde_json::json!(info.level));
                players.push(Player {
                    name: info.name.clone(),
                    steam_id: Some(pu.clone()),
                    team: *t,
                    hero_or_agent: None,
                    stats,
                });
            } else {
                players.push(Player {
                    name: pu.chars().take(6).collect(),
                    steam_id: Some(pu.clone()),
                    team: *t,
                    hero_or_agent: None,
                    stats,
                });
            }
        }
        players
    }
}
