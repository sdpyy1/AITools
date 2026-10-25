use super::types::*;
use crate::core::state::*;

/// 将 CS2 GSI 原始数据解析为统一状态模型
pub fn parse_cs2_state(raw: &Cs2GameState) -> UnifiedGameState {
    let mut state = UnifiedGameState::new(GameType::Cs2);

    // 解析地图信息
    if let Some(map) = &raw.map {
        state.map_or_arena = map.name.clone();
        state.game_mode = map.mode.clone();
        state.round = map.round;
        state.team_ct_score = map.team_ct.as_ref().and_then(|t| t.score);
        state.team_t_score = map.team_t.as_ref().and_then(|t| t.score);

        // 判断对局阶段
        state.match_phase = match map.phase.as_deref() {
            Some("warmup") => MatchPhase::Warmup,
            Some("live") => MatchPhase::Live,
            Some("intermission") => MatchPhase::Intermission,
            Some("gameover") => MatchPhase::GameOver,
            _ => MatchPhase::Unknown,
        };

        state.is_in_match = state.match_phase == MatchPhase::Live
            || state.match_phase == MatchPhase::Warmup
            || state.match_phase == MatchPhase::Intermission;
    }

    // 解析回合信息
    if let Some(round) = &raw.round {
        state.round_phase = round.phase.clone();
    }

    // 解析本地玩家
    if let Some(player) = &raw.player {
        state.local_player = Some(parse_player(player));
    }

    // 解析所有玩家
    if let Some(allplayers) = &raw.allplayers {
        state.players = allplayers.values().map(|p| parse_player(p)).collect();
    }

    // 原始数据透传
    state.raw_payload = Some(serde_json::to_value(raw).unwrap_or_default());

    state
}

fn parse_player(p: &PlayerInfo) -> Player {
    let team = match p.team.as_deref() {
        Some("CT") => Team::Ct,
        Some("T") | Some("TERRORIST") => Team::Terrorist,
        _ => Team::Unknown,
    };

    let mut stats = PlayerStats::default();
    if let Some(s) = &p.state {
        stats.health = s.health;
        stats.armor = s.armor;
        stats.money = s.money;
        stats.round_kills = s.round_kills;
        stats.round_killhs = s.round_killhs;
    }
    if let Some(ms) = &p.match_stats {
        stats.kills = ms.kills;
        stats.deaths = ms.deaths;
        stats.assists = ms.assists;
        if let Some(mvps) = ms.mvps {
            stats
                .extra
                .insert("mvps".to_string(), serde_json::json!(mvps));
        }
    }

    // 收集活跃武器信息作为 hero_or_agent
    let active_weapon = p.weapons.as_ref().and_then(|weapons| {
        weapons
            .values()
            .find(|w| w.state.as_deref() == Some("active"))
            .and_then(|w| w.name.clone())
    });

    Player {
        name: p.name.clone().unwrap_or_default(),
        steam_id: p.steamid.clone(),
        team,
        hero_or_agent: active_weapon,
        stats,
    }
}
