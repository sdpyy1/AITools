use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{error, info, warn};

/// GSI 配置模板 — 开启所有可用数据块，后端拿全量数据，前端按需展示
const GSI_CONFIG_CONTENT: &str = r#""GameStateIntegration"
{
    "uri"           "http://127.0.0.1:3001/gsi/cs2"
    "timeout"       "5.0"
    "buffer"        "0.1"
    "throttle"      "0.1"
    "heartbeat"     "30.0"
    "data"
    {
        "provider"              "1"
        "map"                   "1"
        "round"                 "1"
        "player_id"             "1"
        "player_state"          "1"
        "player_weapons"        "1"
        "player_match_stats"    "1"
        "allplayers_id"         "1"
        "allplayers_state"      "1"
        "allplayers_match_stats" "1"
        "allplayers_weapons"    "1"
        "allplayers_position"   "1"
        "phase_countdowns"      "1"
        "allgrenades"           "1"
        "bomb"                  "1"
    }
}
"#;

const CONFIG_FILENAME: &str = "gamestate_integration_bridge.cfg";

/// 自动配置结果
#[derive(Debug, Clone)]
pub struct AutoConfigResult {
    pub success: bool,
    pub message: String,
    pub config_path: Option<PathBuf>,
}

impl AutoConfigResult {
    fn success(path: PathBuf) -> Self {
        Self {
            success: true,
            message: format!("配置文件已写入: {}", path.display()),
            config_path: Some(path),
        }
    }

    fn failure(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            message: msg.into(),
            config_path: None,
        }
    }
}

/// 一键自动配置：检测 CS2 目录并写入 GSI 配置文件
pub fn auto_configure_cs2() -> AutoConfigResult {
    info!("Starting CS2 auto-configuration...");

    let steam_path = match find_steam_install_path() {
        Some(path) => {
            info!("Found Steam install path: {}", path.display());
            path
        }
        None => {
            warn!("Could not find Steam via registry, trying common paths");
            return try_common_paths();
        }
    };

    let library_folders = match parse_library_folders(&steam_path) {
        Ok(folders) => folders,
        Err(e) => {
            warn!(
                "Failed to parse libraryfolders.vdf: {}, trying common paths",
                e
            );
            return try_common_paths();
        }
    };

    for folder in library_folders {
        let cs2_path =
            folder.join("steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg");
        if cs2_path.exists() {
            info!("Found CS2 config directory: {}", cs2_path.display());
            return write_config(&cs2_path);
        }
    }

    try_common_paths()
}

/// 通过注册表查找 Steam 安装路径
fn find_steam_install_path() -> Option<PathBuf> {
    if let Some(path) = query_registry(r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath") {
        return Some(path);
    }
    if let Some(path) = query_registry(r"HKLM\SOFTWARE\Valve\Steam", "InstallPath") {
        return Some(path);
    }
    if let Some(path) = query_registry(r"HKCU\Software\Valve\Steam", "SteamPath") {
        return Some(path);
    }
    None
}

fn query_registry(key: &str, value_name: &str) -> Option<PathBuf> {
    let output = Command::new("reg")
        .args(["query", key, "/v", value_name])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if line.contains(value_name) && line.contains("REG_SZ") {
            let value_start = line.find("REG_SZ").unwrap_or(0) + "REG_SZ".len();
            let value = line[value_start..].trim();
            if !value.is_empty() {
                let path = PathBuf::from(value);
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// 解析 Steam 的 libraryfolders.vdf
fn parse_library_folders(steam_path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let vdf_path = steam_path.join("steamapps/libraryfolders.vdf");

    if !vdf_path.exists() {
        anyhow::bail!("libraryfolders.vdf not found");
    }

    let content = fs::read_to_string(&vdf_path)?;
    let mut folders = Vec::new();
    folders.push(steam_path.to_path_buf());

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('"') && trimmed.contains("path") {
            if let Some(start) = trimmed.find('"') {
                let after_first_quote = &trimmed[start + 1..];
                if let Some(end) = after_first_quote.find('"') {
                    let key = &after_first_quote[..end];
                    if key == "path" {
                        let rest = &after_first_quote[end + 1..];
                        if let Some(val_start) = rest.find('"') {
                            let after_val_quote = &rest[val_start + 1..];
                            if let Some(val_end) = after_val_quote.find('"') {
                                let path_str = &after_val_quote[..val_end];
                                let path_str = path_str.replace("\\\\", "\\");
                                let path = PathBuf::from(&path_str);
                                if path.exists() && !folders.contains(&path) {
                                    folders.push(path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(folders)
}

fn try_common_paths() -> AutoConfigResult {
    let common_paths = [
        r"C:\Program Files (x86)\Steam\steamapps\common\Counter-Strike Global Offensive\game\csgo\cfg",
        r"C:\Program Files\Steam\steamapps\common\Counter-Strike Global Offensive\game\csgo\cfg",
        r"D:\Steam\steamapps\common\Counter-Strike Global Offensive\game\csgo\cfg",
        r"D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive\game\csgo\cfg",
        r"E:\Steam\steamapps\common\Counter-Strike Global Offensive\game\csgo\cfg",
        r"E:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive\game\csgo\cfg",
    ];

    for path_str in &common_paths {
        let path = PathBuf::from(path_str);
        if path.exists() {
            return write_config(&path);
        }
    }

    AutoConfigResult::failure(
        "无法自动找到 CS2 安装目录。请手动在游戏 cfg 目录创建 gamestate_integration_bridge.cfg",
    )
}

fn write_config(cfg_dir: &Path) -> AutoConfigResult {
    let config_path = cfg_dir.join(CONFIG_FILENAME);

    match fs::write(&config_path, GSI_CONFIG_CONTENT) {
        Ok(()) => {
            info!("Successfully wrote GSI config to {}", config_path.display());
            AutoConfigResult::success(config_path)
        }
        Err(e) => {
            error!("Failed to write config: {}", e);
            AutoConfigResult::failure(format!("写入失败: {}。请尝试以管理员身份运行", e))
        }
    }
}

/// 检查配置文件是否已存在
pub fn is_configured() -> Option<PathBuf> {
    let steam_path = find_steam_install_path();

    let search_paths: Vec<PathBuf> = if let Some(ref path) = steam_path {
        parse_library_folders(path).unwrap_or_else(|_| vec![path.clone()])
    } else {
        vec![]
    };

    for folder in &search_paths {
        let p1 = folder
            .join("steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg")
            .join(CONFIG_FILENAME);
        let p2 = folder.join(CONFIG_FILENAME);
        if p1.exists() {
            return Some(p1);
        }
        if p2.exists() {
            return Some(p2);
        }
    }

    // fallback
    for path_str in [
        r"C:\Program Files (x86)\Steam\steamapps\common\Counter-Strike Global Offensive\game\csgo\cfg",
        r"D:\Steam\steamapps\common\Counter-Strike Global Offensive\game\csgo\cfg",
    ] {
        let p = PathBuf::from(path_str).join(CONFIG_FILENAME);
        if p.exists() {
            return Some(p);
        }
    }

    None
}
