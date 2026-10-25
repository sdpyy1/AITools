//! LOL 业务指令：一键匹配、上等马评选、发送聊天

use crate::adapters::lol::lcu::LcuClient;
use crate::adapters::lol::live::LiveClient;
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;

/// 战绩评分时回看的近期对局场数
const RECENT_GAMES: u32 = 20;

/// 根据配置解析目标队列 ID（显式 queue_id 优先，否则按名称子串匹配）
pub async fn resolve_queue_id(
    client: &LcuClient,
    mode_name: &str,
    explicit: Option<u32>,
) -> Result<u32> {
    if let Some(q) = explicit {
        return Ok(q);
    }
    let queues = client.game_queues().await?;
    let arr = queues
        .as_array()
        .ok_or_else(|| anyhow!("game queues 响应不是数组"))?;
    let target = mode_name.to_lowercase();
    let mut available_match: Option<u32> = None;
    let mut any_match: Option<u32> = None;
    for q in arr {
        let name = q.get("name").and_then(|x| x.as_str()).unwrap_or("");
        let short = q.get("shortName").and_then(|x| x.as_str()).unwrap_or("");
        if name.to_lowercase().contains(&target) || short.to_lowercase().contains(&target) {
            let id = match q.get("id").and_then(|x| x.as_u64()) {
                Some(v) => v as u32,
                None => continue,
            };
            if any_match.is_none() {
                any_match = Some(id);
            }
            // 只在当前可玩的队列里选：依赖 queueAvailability == "Available"（国服客户端该字段可靠，
            // enabledForExplicitPick 等字段为 null 不可信）
            let avail = q
                .get("queueAvailability")
                .and_then(|x| x.as_str())
                .unwrap_or("Available");
            if avail == "Available" {
                available_match = Some(id);
                break;
            }
        }
    }
    available_match
        .or(any_match)
        .ok_or_else(|| {
            anyhow!(
                "未找到匹配 '{}' 的队列，请检查 mode_name 或直接在配置里设置 queue_id",
                mode_name
            )
        })
}

#[derive(Debug, Serialize, Clone)]
pub struct PlayerInfo {
    pub name: String,
    pub level: i64,
    pub score: i64,
    /// 段位展示（如 "翡翠 I 45 (58%WR)"）
    pub tier_label: String,
    /// 近期战绩展示（如 "近20场 60%WR KDA2.1"）
    pub perf_label: String,
    /// 近 5 场 KDA 明细，如 "(5/10/14/负) (2/3/8/胜)"
    pub recent_detail: String,
    /// 上等马 / 中等马 / 下等马（按战力排名自动分档）
    pub horse: String,
    pub puuid: String,
}

pub struct HorseEval {
    pub players: Vec<PlayerInfo>,
    /// 聊天消息（逐行：名字 战绩信息， xx等马）
    pub message: String,
    /// 面板逐行展示用
    pub lines: Vec<String>,
}

/// 读取对局内本地玩家的实时状态（存活/阵亡/复活倒计时/实时KDA/英雄）。
/// 来自 Live Client Data API（/liveclientdata/allgamedata）。
/// 注意：isDead/respawnTimer/championName/scores 只在 allPlayers[] 里，activePlayer 没有，
/// 需要用 activePlayer.summonerName 去 allPlayers 匹配本地玩家。
/// 不在对局中时返回 in_game=false。
pub async fn live_status(client: &LiveClient) -> Result<Value> {
    let data = client.all_game_data().await?;
    let active = data.get("activePlayer");
    let local = client.local_player().await?;
    let dead = local
        .get("isDead")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let respawn = local
        .get("respawnTimer")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    let level = local
        .get("level")
        .and_then(|x| x.as_i64())
        .or_else(|| active.and_then(|p| p.get("level")).and_then(|x| x.as_i64()))
        .unwrap_or(0);
    let gold = active
        .and_then(|p| p.get("currentGold"))
        .or_else(|| active.and_then(|p| p.get("gold")))
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    let champion = local
        .get("championName")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let (kills, deaths, assists) = local
        .get("scores")
        .map(|s| {
            (
                s.get("kills").and_then(|x| x.as_i64()).unwrap_or(0),
                s.get("deaths").and_then(|x| x.as_i64()).unwrap_or(0),
                s.get("assists").and_then(|x| x.as_i64()).unwrap_or(0),
            )
        })
        .unwrap_or((0, 0, 0));
    Ok(serde_json::json!({
        "in_game": true,
        "dead": dead,
        "respawn_timer": respawn,
        "level": level,
        "gold": gold as i64,
        "champion": champion,
        "kills": kills,
        "deaths": deaths,
        "assists": assists,
    }))
}

/// 上等马评选（统一入口）：自动判断数据来源——
/// 优先英雄选择会话（ChampSelect），其次对局内会话（InProgress，从 gameflow 取 teamOne/teamTwo）。
pub async fn evaluate(client: &LcuClient) -> Result<HorseEval> {
    let puuids = collect_puuids(client).await?;
    evaluate_from_puuids(client, &puuids).await
}

/// 收集当前对局/英雄选择里的全部玩家 puuid。
async fn collect_puuids(client: &LcuClient) -> Result<Vec<String>> {
    // 1) 英雄选择阶段
    if let Ok(sess) = client.champ_select_session().await {
        let puuids = session_puuids(&sess);
        if !puuids.is_empty() {
            return Ok(puuids);
        }
    }
    // 2) 对局内进行中（InProgress）：从 gameflow 会话拿 teamOne/teamTwo
    let sess = client.gameflow_session().await?;
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
    let puuids = t1
        .iter()
        .chain(t2.iter())
        .filter_map(|p| p.get("puuid").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if puuids.is_empty() {
        anyhow::bail!("未在对局/英雄选择中，无法获取玩家列表");
    }
    Ok(puuids)
}

fn session_puuids(sess: &Value) -> Vec<String> {
    let my = sess.get("myTeam").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let their = sess.get("theirTeam").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    my.into_iter()
        .chain(their.into_iter())
        .filter_map(|m| m.get("puuid").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// 根据 puuid 列表完成战绩拉取、排序、分档，生成消息与逐行内容。
pub(crate) async fn evaluate_from_puuids(
    client: &LcuClient,
    puuids: &[String],
) -> Result<HorseEval> {
    let map = enrich_players_map(client, puuids).await;
    let mut players: Vec<PlayerInfo> = map.into_values().collect();
    players.sort_by(|a, b| b.score.cmp(&a.score));
    classify(&mut players);
    let lines = build_lines(&players);
    let message = lines.join("\n");
    Ok(HorseEval {
        players,
        message,
        lines,
    })
}

/// 按战力排序后三等分：前 1/3 上等马、后 1/3 下等马、中间中等马。
fn classify(players: &mut [PlayerInfo]) {
    let n = players.len();
    if n == 0 {
        return;
    }
    let upper = (n / 3).max(1);
    let lower = (n / 3).max(1);
    for (i, p) in players.iter_mut().enumerate() {
        p.horse = if i < upper {
            "上等马".into()
        } else if i >= n - lower {
            "下等马".into()
        } else {
            "中等马".into()
        };
    }
}

/// 并行拉取一组玩家的战绩（段位 + 近期 KDA/胜率），以 puuid 为键返回。
/// 供上等马评选与对局状态面板（ChampSelect / InProgress）共用，避免重复代码。
pub(crate) async fn enrich_players_map(
    client: &LcuClient,
    puuids: &[String],
) -> std::collections::HashMap<String, PlayerInfo> {
    let results = futures::future::join_all(puuids.iter().map(|p| fetch_player_score(client, p))).await;
    let mut map = std::collections::HashMap::new();
    for info in results {
        map.insert(info.puuid.clone(), info);
    }
    map
}

async fn fetch_player_score(client: &LcuClient, puuid: &str) -> PlayerInfo {
    let summoner = client.summoner_by_puuid(puuid).await.ok();
    let name = summoner
        .as_ref()
        .and_then(|v| {
            v.get("gameName")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .or_else(|| v.get("displayName").and_then(|x| x.as_str()).map(|s| s.to_string()))
        })
        .unwrap_or_else(|| puuid.chars().take(6).collect());
    let level = summoner
        .as_ref()
        .and_then(|v| v.get("summonerLevel").and_then(|x| x.as_i64()))
        .unwrap_or(0);
    let ranked = client.ranked_stats(puuid).await.ok();
    let (rank_score, tier_label) = compute_rank_score(ranked);
    // 参考 lcu-driver 脚本：拉近期对局，按 KDA + 胜率算分
    let (perf_score, perf_label, recent_detail) =
        compute_perf_score(client.match_history(puuid, RECENT_GAMES).await.ok());
    PlayerInfo {
        name,
        level,
        score: rank_score + perf_score,
        tier_label,
        perf_label,
        recent_detail,
        horse: String::new(),
        puuid: puuid.to_string(),
    }
}

fn tier_rank(tier: &str) -> i64 {
    match tier.to_uppercase().as_str() {
        "IRON" => 0,
        "BRONZE" => 1,
        "SILVER" => 2,
        "GOLD" => 3,
        "PLATINUM" => 4,
        "EMERALD" => 5,
        "DIAMOND" => 6,
        "MASTER" => 7,
        "GRANDMASTER" => 8,
        "CHALLENGER" => 9,
        _ => -1,
    }
}

fn division_rank(div: &str) -> i64 {
    match div.to_uppercase().as_str() {
        "I" => 3,
        "II" => 2,
        "III" => 1,
        "IV" => 0,
        _ => 0,
    }
}

/// 取玩家所有队列里的最高战力评分（段位×1000 + 小段×200 + LP）。
/// 段位仍是“上等马”的主导因素，近期战绩作为叠加修正。
fn compute_rank_score(ranked: Option<Value>) -> (i64, String) {
    let mut best: (i64, String) = (0, "未定级".into());
    if let Some(v) = ranked {
        if let Some(qs) = v.get("queues").and_then(|x| x.as_array()) {
            for q in qs {
                let tier = q.get("tier").and_then(|x| x.as_str()).unwrap_or("");
                let div = q.get("division").and_then(|x| x.as_str()).unwrap_or("");
                if tier_rank(tier) < 0 {
                    continue;
                }
                let lp = q.get("leaguePoints").and_then(|x| x.as_i64()).unwrap_or(0);
                let wins = q.get("wins").and_then(|x| x.as_i64()).unwrap_or(0);
                let losses = q.get("losses").and_then(|x| x.as_i64()).unwrap_or(0);
                let total = wins + losses;
                let wr = if total > 0 { wins * 100 / total } else { 0 };
                let score = tier_rank(tier) * 1000 + division_rank(div) * 200 + lp;
                if score > best.0 {
                    best = (
                        score,
                        format!("{}{} {} ({}%WR)", tier, div, lp, wr),
                    );
                }
            }
        }
    }
    best
}

/// 从近期对局（match-history）计算 KDA 与胜率评分，并生成近 5 场 KDA 明细。
/// 参考 lcu-driver 脚本的算法：胜场 + KDA 加权。
fn compute_perf_score(games: Option<Value>) -> (i64, String, String) {
    let mut wins = 0i64;
    let mut total = 0i64;
    let mut kda_sum = 0.0f64;
    let mut detail = String::new();
    if let Some(v) = &games {
        if let Some(arr) = v
            .get("games")
            .and_then(|x| x.get("games"))
            .and_then(|x| x.as_array())
        {
            for g in arr {
                let stats = g
                    .get("participants")
                    .and_then(|x| x.as_array())
                    .and_then(|a| a.first())
                    .and_then(|p| p.get("stats"));
                if let Some(s) = stats {
                    let k = s.get("kills").and_then(|x| x.as_f64()).unwrap_or(0.0);
                    let d = s.get("deaths").and_then(|x| x.as_f64()).unwrap_or(0.0);
                    let a = s.get("assists").and_then(|x| x.as_f64()).unwrap_or(0.0);
                    let win = s.get("win").and_then(|x| x.as_bool()).unwrap_or(false);
                    total += 1;
                    if win {
                        wins += 1;
                    }
                    kda_sum += (k + a) / d.max(1.0);
                    // 近 5 场明细，如 (5/10/14/负)
                    if total <= 5 {
                        if !detail.is_empty() {
                            detail.push(' ');
                        }
                        detail.push_str(&format!(
                            "({}/{}/{}/{}",
                            k as i64,
                            d as i64,
                            a as i64,
                            if win { "胜" } else { "负" }
                        ));
                        detail.push(')');
                    }
                }
            }
        }
    }
    if total == 0 {
        return (0, "无近期战绩".into(), String::new());
    }
    let wr = (wins * 100 / total) as i64;
    let avg_kda = kda_sum / total as f64;
    // 评分：胜场×40 + 高于基准的 KDA×80 + 高于 50% 的胜率×2
    let perf_score = wins * 40
        + (((avg_kda - 1.0).max(0.0) * 80.0) as i64)
        + ((wr - 50).max(0) * 2);
    (
        perf_score,
        format!("近{}场 {}%WR KDA{:.1}", total, wr, avg_kda),
        detail,
    )
}

/// 逐行展示：名字  段位·近期战绩，  X等马
fn build_lines(players: &[PlayerInfo]) -> Vec<String> {
    let n = players.len();
    if n == 0 {
        return vec!["暂无玩家数据".into()];
    }
    players
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let perf = if p.perf_label.is_empty() {
                String::new()
            } else {
                format!(" {}", p.perf_label)
            };
            format!(
                "{}. {}  {}{}，  {}",
                i + 1,
                p.name,
                p.tier_label,
                perf,
                p.horse
            )
        })
        .collect()
}

fn build_message(players: &[PlayerInfo]) -> String {
    build_lines(players).join("\n")
}

/// 找到游戏内/英雄选择聊天会话并发送消息（尽力发到 all-chat）
pub async fn send_all_chat(client: &LcuClient, msg: &str) -> Result<()> {
    let convs = client.conversations().await?;
    let arr = convs
        .as_array()
        .ok_or_else(|| anyhow!("conversations 响应不是数组"))?;
    let id = arr
        .iter()
        .filter_map(|c| {
            let t = c.get("type").and_then(|x| x.as_str()).unwrap_or("");
            let id = c.get("id").and_then(|x| x.as_str());
            match t {
                "game" | "championSelect" => id,
                _ => None,
            }
        })
        .next()
        .ok_or_else(|| anyhow!("未找到游戏内聊天会话"))?;
    client.send_chat_message(id, msg).await?;
    Ok(())
}
