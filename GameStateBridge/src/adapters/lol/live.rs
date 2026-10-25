//! Live Client Data API 客户端（游戏内实时数据）
//!
//! 地址：https://127.0.0.1:2999/liveclientdata/... （部分版本为 34243）
//! 自签证书 + 老 TLS，禁用证书校验、绕过代理即可（Windows 下走 Schannel 兼容良好）。
//! 用于检测本地玩家的生死状态（activePlayer.isDead / respawnTimer）。

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

pub struct LiveClient {
    client: Client,
    base: String,
}

impl LiveClient {
    /// 优先 2999，可用环境变量 LOL_LIVE_PORT 覆盖
    pub fn new() -> Result<Self> {
        let port: u16 = std::env::var("LOL_LIVE_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2999);
        let client = Client::builder()
            .timeout(Duration::from_secs(3))
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .build()
            .context("构建 Live API 客户端失败")?;
        Ok(Self {
            client,
            base: format!("https://127.0.0.1:{}", port),
        })
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        let resp = self.client.get(&url).send().await.context("Live API 请求失败")?;
        if resp.status().as_u16() == 404 {
            anyhow::bail!("Live API 404（客户端未在对局中）");
        }
        let text = resp.text().await.unwrap_or_default();
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).with_context(|| format!("解析 Live API 失败: {}", path))
    }

    /// 全量对局数据（含 activePlayer / allPlayers / events / gameData）
    pub async fn all_game_data(&self) -> Result<Value> {
        self.get("/liveclientdata/allgamedata").await
    }

    /// 客户端是否处于对局中（API 可达）
    pub async fn is_running(&self) -> bool {
        self.get("/liveclientdata/activeplayername").await.is_ok()
    }

    /// 当前本地玩家是否已阵亡
    pub async fn is_dead(&self) -> Result<bool> {
        let p = self.local_player().await?;
        Ok(p.get("isDead").and_then(|x| x.as_bool()).unwrap_or(false))
    }

    /// 在 allPlayers 中定位“本地玩家”条目。
    /// 注意：activePlayer 里没有 isDead/respawnTimer/championName，这些只在 allPlayers[] 里；
    /// 用 activePlayer.summonerName（含 #Tag，国服格式如 "名字#86354"）去 allPlayers 精确匹配。
    pub async fn local_player(&self) -> Result<Value> {
        let data = self.all_game_data().await?;
        let name = data
            .get("activePlayer")
            .and_then(|p| p.get("summonerName"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let players = data
            .get("allPlayers")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        for p in players {
            if p.get("summonerName").and_then(|x| x.as_str()) == Some(name) {
                return Ok(p);
            }
        }
        anyhow::bail!("未在对局中找到本地玩家（summonerName={}）", name)
    }
}
