use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// CS2 GSI 顶层 payload
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Cs2GameState {
    #[serde(default)]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub map: Option<MapInfo>,
    #[serde(default)]
    pub round: Option<RoundInfo>,
    #[serde(default)]
    pub player: Option<PlayerInfo>,
    #[serde(default)]
    pub allplayers: Option<HashMap<String, PlayerInfo>>,
    #[serde(default)]
    pub previously: Option<serde_json::Value>,
    #[serde(default)]
    pub added: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Provider {
    pub name: Option<String>,
    pub appid: Option<i64>,
    pub version: Option<i64>,
    pub steamid: Option<String>,
    pub timestamp: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MapInfo {
    pub mode: Option<String>,
    pub name: Option<String>,
    pub phase: Option<String>,
    pub round: Option<i32>,
    #[serde(rename = "team_ct")]
    pub team_ct: Option<TeamInfo>,
    #[serde(rename = "team_t")]
    pub team_t: Option<TeamInfo>,
    #[serde(rename = "num_matches_to_win_series")]
    pub num_matches_to_win_series: Option<i32>,
    #[serde(rename = "current_spectators")]
    pub current_spectators: Option<i32>,
    #[serde(rename = "souvenirs_total")]
    pub souvenirs_total: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TeamInfo {
    pub score: Option<i32>,
    pub name: Option<String>,
    #[serde(rename = "consecutive_round_losses")]
    pub consecutive_round_losses: Option<i32>,
    #[serde(rename = "timeouts_remaining")]
    pub timeouts_remaining: Option<i32>,
    #[serde(rename = "matches_won_this_series")]
    pub matches_won_this_series: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RoundInfo {
    pub phase: Option<String>,
    #[serde(rename = "win_team")]
    pub win_team: Option<String>,
    pub bomb: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PlayerInfo {
    pub steamid: Option<String>,
    pub name: Option<String>,
    pub team: Option<String>,
    pub activity: Option<String>,
    #[serde(default)]
    pub state: Option<PlayerState>,
    #[serde(default)]
    pub weapons: Option<HashMap<String, WeaponInfo>>,
    #[serde(rename = "match_stats", default)]
    pub match_stats: Option<MatchStats>,
    #[serde(rename = "spectarget", default)]
    pub spectarget: Option<String>,
    #[serde(rename = "position", default)]
    pub position: Option<String>,
    #[serde(rename = "forward", default)]
    pub forward: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PlayerState {
    pub health: Option<i32>,
    pub armor: Option<i32>,
    pub helmet: Option<bool>,
    pub flashed: Option<i32>,
    pub smoked: Option<i32>,
    pub burning: Option<i32>,
    pub money: Option<i32>,
    #[serde(rename = "round_kills")]
    pub round_kills: Option<i32>,
    #[serde(rename = "round_killhs")]
    pub round_killhs: Option<i32>,
    #[serde(rename = "equip_value")]
    pub equip_value: Option<i32>,
    #[serde(rename = "defusekit", default)]
    pub defusekit: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MatchStats {
    pub kills: Option<i32>,
    pub assists: Option<i32>,
    pub deaths: Option<i32>,
    pub mvps: Option<i32>,
    pub score: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WeaponInfo {
    pub name: Option<String>,
    #[serde(rename = "paintkit")]
    pub paintkit: Option<String>,
    #[serde(rename = "type")]
    pub weapon_type: Option<String>,
    #[serde(default)]
    pub ammo_clip: Option<i32>,
    #[serde(default)]
    pub ammo_clip_max: Option<i32>,
    #[serde(default)]
    pub ammo_reserve: Option<i32>,
    pub state: Option<String>,
}
