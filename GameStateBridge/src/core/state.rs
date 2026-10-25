use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 支持的游戏类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameType {
    Cs2,
    Lol,
}

impl GameType {
    pub fn as_str(&self) -> &'static str {
        match self {
            GameType::Cs2 => "cs2",
            GameType::Lol => "lol",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            GameType::Cs2 => "CS2",
            GameType::Lol => "League of Legends",
        }
    }

    pub fn all() -> Vec<GameType> {
        vec![GameType::Cs2, GameType::Lol]
    }
}

impl std::str::FromStr for GameType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "cs2" | "csgo" => Ok(GameType::Cs2),
            "lol" | "league" | "leagueoflegends" => Ok(GameType::Lol),
            _ => Err(format!("Unknown game type: {}", s)),
        }
    }
}

/// 对局阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchPhase {
    Menu,
    Loading,
    Warmup,
    Live,
    Intermission,
    GameOver,
    Unknown,
}

/// 阵营
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Team {
    Ct,
    Terrorist,
    Blue,
    Red,
    Spectator,
    Unknown,
}

/// 玩家统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerStats {
    pub kills: Option<i32>,
    pub deaths: Option<i32>,
    pub assists: Option<i32>,
    pub health: Option<i32>,
    pub armor: Option<i32>,
    pub money: Option<i32>,
    #[serde(rename = "round_kills")]
    pub round_kills: Option<i32>,
    #[serde(rename = "round_killhs")]
    pub round_killhs: Option<i32>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// 统一玩家模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub steam_id: Option<String>,
    pub team: Team,
    pub hero_or_agent: Option<String>,
    pub stats: PlayerStats,
}

/// 统一对局状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedGameState {
    pub game: GameType,
    pub is_in_match: bool,
    pub match_id: Option<String>,
    pub map_or_arena: Option<String>,
    pub game_mode: Option<String>,
    pub match_phase: MatchPhase,
    pub local_player: Option<Player>,
    pub players: Vec<Player>,
    pub round: Option<i32>,
    pub round_phase: Option<String>,
    pub team_ct_score: Option<i32>,
    pub team_t_score: Option<i32>,
    pub timestamp: DateTime<Utc>,
    /// 原始扩展字段，各游戏特有的额外数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_payload: Option<serde_json::Value>,
}

impl UnifiedGameState {
    pub fn new(game: GameType) -> Self {
        Self {
            game,
            is_in_match: false,
            match_id: None,
            map_or_arena: None,
            game_mode: None,
            match_phase: MatchPhase::Unknown,
            local_player: None,
            players: Vec::new(),
            round: None,
            round_phase: None,
            team_ct_score: None,
            team_t_score: None,
            timestamp: Utc::now(),
            raw_payload: None,
        }
    }
}

/// 适配器状态摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterStatus {
    pub game_type: GameType,
    pub is_initialized: bool,
    pub is_connected: bool,
    pub last_update: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}
