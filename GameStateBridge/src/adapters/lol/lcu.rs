//! LCU (League Client Update) API 客户端
//!
//! 负责：发现本地客户端凭据（lockfile / 国服日志首行 / RiotClientInstalls.json），
//! 通过 Basic riot:<token> 认证访问本地 HTTPS 接口。
//! 所有请求禁用证书校验（自签证书）并绕过系统代理（指向 127.0.0.1）。

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

/// 从 lockfile 或日志中提取的凭据
#[derive(Debug, Clone)]
pub struct Credentials {
    pub port: u16,
    pub token: String,
    pub source: String,
}

/// LCU HTTP 客户端
#[derive(Debug, Clone)]
pub struct LcuClient {
    client: Client,
    base: String,
    creds: Credentials,
}

impl LcuClient {
    /// 自动发现凭据并建立客户端
    pub fn discover() -> Result<Self> {
        let creds = discover_credentials().context("发现 LCU 凭据失败")?;
        Self::with_credentials(creds)
    }

    pub fn with_credentials(creds: Credentials) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(8))
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .build()
            .context("构建 LCU HTTP 客户端失败")?;
        let base = format!("https://127.0.0.1:{}", creds.port);
        Ok(Self {
            client,
            base,
            creds,
        })
    }

    pub fn credentials(&self) -> &Credentials {
        &self.creds
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        let mut req = self
            .client
            .request(method.clone(), &url)
            .basic_auth("riot", Some(&self.creds.token));
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.context("LCU 请求发送失败")?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "LCU {} {} -> {}: {}",
                method,
                path,
                status,
                txt.chars().take(200).collect::<String>()
            );
        }
        let text = resp.text().await.unwrap_or_default();
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).with_context(|| format!("解析 LCU 响应失败: {}", path))
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        self.request(reqwest::Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.request(reqwest::Method::POST, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: Value) -> Result<Value> {
        self.request(reqwest::Method::PUT, path, Some(body)).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        self.request(reqwest::Method::DELETE, path, None).await
    }

    // ---------------- 业务端点 ----------------

    pub async fn gameflow_phase(&self) -> Result<String> {
        let v = self.get("/lol-gameflow/v1/gameflow-phase").await?;
        Ok(v.as_str().unwrap_or("Unknown").to_string())
    }

    pub async fn game_queues(&self) -> Result<Value> {
        self.get("/lol-game-queues/v1/queues").await
    }

    /// 创建指定队列的房间（一键匹配的核心）。
    /// 普通队列只需 queueId，partyType 由 LCU 按队列配置自动决定；
    /// 显式传 open/closed 反而会在 ARAM/海克斯等队列上触发 INVALID_LOBBY。
    pub async fn create_lobby(&self, queue_id: u32) -> Result<Value> {
        self.post("/lol-lobby/v2/lobby", json!({ "queueId": queue_id }))
            .await
    }

    /// 查询当前是否已有大厅
    pub async fn get_lobby(&self) -> Result<Value> {
        self.get("/lol-lobby/v2/lobby").await
    }

    /// 退出当前大厅（重建前清场，避免 INVALID_LOBBY）
    pub async fn leave_lobby(&self) -> Result<Value> {
        self.delete("/lol-lobby/v2/lobby").await
    }

    /// 开始匹配搜索
    pub async fn start_matchmaking(&self) -> Result<Value> {
        self.post("/lol-lobby/v2/lobby/matchmaking/search", json!({}))
            .await
    }

    /// 取消匹配
    pub async fn cancel_matchmaking(&self) -> Result<Value> {
        self.delete("/lol-lobby/v2/lobby/matchmaking/search").await
    }

    /// 查询当前 ready-check 状态（state: InProgress=待接受 / Accepted / Declined ...）
    pub async fn ready_check(&self) -> Result<Value> {
        self.get("/lol-matchmaking/v1/ready-check").await
    }

    /// 接受对局（在 ready-check 处于 InProgress 时点击"接受"）
    pub async fn accept_ready_check(&self) -> Result<Value> {
        self.post("/lol-matchmaking/v1/ready-check/accept", json!({}))
            .await
    }

    pub async fn champ_select_session(&self) -> Result<Value> {
        self.get("/lol-champ-select/v1/session").await
    }

    /// 当前游戏流程会话（InProgress 时含 teamOne/teamTwo 的 puuid，用于拉对局内玩家战绩）
    pub async fn gameflow_session(&self) -> Result<Value> {
        self.get("/lol-gameflow/v1/session").await
    }

    /// 当前登录玩家的召唤师信息（含 puuid）
    pub async fn summoner_current(&self) -> Result<Value> {
        self.get("/lol-summoner/v1/current-summoner").await
    }

    pub async fn summoner_by_puuid(&self, puuid: &str) -> Result<Value> {
        // 国服客户端 v1 by-puuid 返回 404，需用 v2 /summoners/puuid（参考 lcu-driver 脚本）
        match self
            .get(&format!("/lol-summoner/v2/summoners/puuid/{}", puuid))
            .await
        {
            Ok(v) => Ok(v),
            Err(_) => {
                self.get(&format!("/lol-summoner/v1/summoners/by-puuid/{}", puuid))
                    .await
            }
        }
    }

    pub async fn ranked_stats(&self, puuid: &str) -> Result<Value> {
        self.get(&format!("/lol-ranked/v1/ranked-stats/{}", puuid))
            .await
    }

    /// 拉取玩家最近 N 场对局（用于战绩/KDA 评分）。
    /// 端点参考：/lol-match-history/v1/products/lol/{puuid}/matches?begIndex=0&endIndex={count-1}
    pub async fn match_history(&self, puuid: &str, count: u32) -> Result<Value> {
        self.get(&format!(
            "/lol-match-history/v1/products/lol/{}/matches?begIndex=0&endIndex={}",
            puuid,
            count.saturating_sub(1)
        ))
        .await
    }

    pub async fn conversations(&self) -> Result<Value> {
        self.get("/lol-chat/v1/conversations").await
    }

    pub async fn send_chat_message(&self, conversation_id: &str, body: &str) -> Result<Value> {
        self.post(
            &format!("/lol-chat/v1/conversations/{}/messages", conversation_id),
            json!({ "body": body, "type": "chat" }),
        )
        .await
    }
}

// ---------------- 凭据发现 ----------------

pub fn discover_credentials() -> Result<Credentials> {
    // 1. 通过进程路径定位安装目录，读 lockfile / 日志
    if let Some(dir) = find_install_dir() {
        if let Some(c) = read_lockfile(&dir) {
            return Ok(c);
        }
        if let Some(c) = read_log_credentials(&dir) {
            return Ok(c);
        }
    }
    // 2. 通过 RiotClientInstalls.json 定位安装目录
    if let Some(c) = discover_via_installs_json() {
        return Ok(c);
    }
    anyhow::bail!("无法发现 LCU 凭据，请确认英雄联盟客户端已登录并运行")
}

/// 原生 Win32 枚举进程并查询 exe 路径，取 LeagueClient(Ux) 的安装目录。
/// 不用 PowerShell：① 提权的 LeagueClient 进程 Get-Process 读不到 Path；
/// ② 中文安装路径（如 e:\wegameapps\英雄联盟）经控制台编码会乱码。
#[cfg(windows)]
fn find_install_dir() -> Option<PathBuf> {
    use std::mem;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const TARGETS: [&str; 2] = ["leagueclientux.exe", "leagueclient.exe"];

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found: Option<(u32, bool)> = None; // (pid, is_ux)
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let name = String::from_utf16_lossy(&entry.szExeFile);
                let name = name.trim_end_matches('\0').to_lowercase();
                if TARGETS.contains(&name.as_str()) {
                    let pid = entry.th32ProcessID;
                    let is_ux = name == "leagueclientux.exe";
                    match found {
                        // 优先 Ux（它自己就是 LCU 服务端，且未被提权保护）
                        Some((_, true)) => {}
                        Some((_, false)) if is_ux => found = Some((pid, true)),
                        None => found = Some((pid, is_ux)),
                        _ => {}
                    }
                    if matches!(found, Some((_, true))) {
                        break;
                    }
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        let pid = found?.0;

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 || len == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        PathBuf::from(path).parent().map(|p| p.to_path_buf())
    }
}

#[cfg(not(windows))]
fn find_install_dir() -> Option<PathBuf> {
    None
}

fn read_lockfile(dir: &PathBuf) -> Option<Credentials> {
    let content = std::fs::read_to_string(dir.join("lockfile")).ok()?;
    let t = content.trim();
    if t.is_empty() {
        return None; // 国服会把 lockfile 清空
    }
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() >= 3 {
        let port: u16 = parts[1].parse().ok()?;
        let token = parts[2].to_string();
        return Some(Credentials {
            port,
            token,
            source: "lockfile".into(),
        });
    }
    None
}

/// 解析日志提取 LCU 凭据。
/// 国服布局：安装目录根下 `*LeagueClientUx.log`，凭据在 `Cef CommandLine` 行
/// （`--app-port=N --remoting-auth-token=XXX`）。
/// 国际服布局：`Game/Logs/LeagueClient Logs/*.log` 首行命令行参数。
/// 注意按"空格分词 + 精确前缀"匹配，避免 `--riotclient-app-port` 子串误匹配。
fn read_log_credentials(dir: &PathBuf) -> Option<Credentials> {
    let mut candidates: Vec<PathBuf> = vec![dir.clone()];
    candidates.push(dir.join("Game").join("Logs").join("LeagueClient Logs"));
    // 国服 LeagueClientUx 日志里 app-log-file-path 指向 <上级>/Game/Logs/LeagueClient Logs
    if let Some(parent) = dir.parent() {
        candidates.push(parent.join("Game").join("Logs").join("LeagueClient Logs"));
    }

    // 收集所有日志文件，按修改时间从新到旧尝试解析
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for cdir in candidates {
        if let Ok(entries) = std::fs::read_dir(&cdir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("log") {
                    if let Ok(m) = e.metadata() {
                        if let Ok(t) = m.modified() {
                            logs.push((t, p));
                        }
                    }
                }
            }
        }
    }
    logs.sort_by(|a, b| b.0.cmp(&a.0));

    for (_, path) in logs.iter().take(20) {
        if let Some(c) = parse_credentials_from_log_file(path) {
            return Some(c);
        }
    }
    None
}

fn parse_credentials_from_log_file(path: &PathBuf) -> Option<Credentials> {
    // 日志中英混编，按字节读入再 lossy 转换：token/port 都是 ASCII，不受中文乱码影响
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines() {
        if !line.contains("--remoting-auth-token=") {
            continue;
        }
        let mut token: Option<String> = None;
        let mut port: Option<u16> = None;
        for tok in line.split_whitespace() {
            if let Some(v) = tok.strip_prefix("--remoting-auth-token=") {
                token = Some(v.to_string());
            } else if let Some(v) = tok.strip_prefix("--app-port=") {
                port = v.parse().ok();
            }
        }
        if let (Some(token), Some(port)) = (token, port) {
            return Some(Credentials {
                port,
                token,
                source: "log".into(),
            });
        }
    }
    None
}

fn discover_via_installs_json() -> Option<Credentials> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let path = PathBuf::from(local)
        .join("Riot Games")
        .join("RiotClientInstalls.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&content).ok()?;
    let install = v
        .get("associated_client")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("rc_default").and_then(|x| x.as_str()))?;
    let dir = PathBuf::from(install);
    read_lockfile(&dir).or_else(|| read_log_credentials(&dir))
}
