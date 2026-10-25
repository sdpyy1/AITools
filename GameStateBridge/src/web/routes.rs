use crate::core::manager::AdapterManager;
use crate::core::state::{AdapterStatus, GameType, UnifiedGameState};
use crate::overlay::OverlayCoordinator;
use crate::overlay::config::OverlayConfig;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// API 错误响应
#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
}

/// 游戏列表响应
#[derive(Serialize)]
pub struct GamesResponse {
    pub games: Vec<GameInfo>,
}

#[derive(Serialize)]
pub struct GameInfo {
    pub game_type: String,
    pub display_name: String,
    pub is_supported: bool,
    pub is_active: bool,
}

/// 激活游戏请求
#[derive(Deserialize)]
pub struct ActivateRequest {
    pub listen_addr: Option<String>,
}

/// 自动配置结果响应
#[derive(Serialize)]
pub struct AutoConfigResponse {
    pub success: bool,
    pub message: String,
    pub config_path: Option<String>,
}

/// 配置状态响应
#[derive(Serialize)]
pub struct ConfigStatusResponse {
    pub is_configured: bool,
    pub config_path: Option<String>,
}

/// Overlay 配置更新请求
#[derive(Deserialize)]
pub struct UpdateOverlayRequest {
    pub enabled: Option<bool>,
    pub url: Option<String>,
    pub player_steam_id: Option<String>,
    pub player_name: Option<String>,
}

/// 构建 API 路由
pub fn api_routes(manager: Arc<AdapterManager>, overlay: OverlayCoordinator) -> Router {
    Router::new()
        .route("/api/games", axum::routing::get(list_games))
        .route(
            "/api/games/{game_type}/activate",
            axum::routing::post(activate_game),
        )
        .route(
            "/api/games/{game_type}/deactivate",
            axum::routing::post(deactivate_game),
        )
        .route(
            "/api/games/{game_type}/state",
            axum::routing::get(get_game_state),
        )
        .route(
            "/api/games/{game_type}/status",
            axum::routing::get(get_game_status),
        )
        .route(
            "/api/games/{game_type}/auto-config",
            axum::routing::post(auto_config_game),
        )
        .route(
            "/api/games/{game_type}/config-status",
            axum::routing::get(config_status),
        )
        .route(
            "/api/games/{game_type}/raw",
            axum::routing::get(get_raw_data),
        )
        .route(
            "/api/games/{game_type}/command",
            axum::routing::post(game_command),
        )
        .route(
            "/api/overlay/config",
            axum::routing::get(get_overlay_config),
        )
        .route(
            "/api/overlay/config",
            axum::routing::post(update_overlay_config),
        )
        .route("/api/overlay/debug", axum::routing::get(get_overlay_debug))
        .route("/api/overlay/show", axum::routing::post(show_overlay))
        .route("/api/overlay/hide", axum::routing::post(hide_overlay))
        .with_state((manager, overlay))
}

/// GET /api/games - 列出所有支持的游戏及其状态
async fn list_games(
    State((manager, _overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
) -> Result<Json<GamesResponse>, (StatusCode, Json<ApiError>)> {
    let supported = manager.supported_games();
    let mut games = Vec::new();

    for game_type in supported {
        let is_active = manager.is_active(game_type).await;
        games.push(GameInfo {
            game_type: game_type.as_str().to_string(),
            display_name: game_type.display_name().to_string(),
            is_supported: true,
            is_active,
        });
    }

    Ok(Json(GamesResponse { games }))
}

/// POST /api/games/:game_type/activate - 激活指定游戏的适配器
async fn activate_game(
    State((manager, _overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
    Path(game_type_str): Path<String>,
    Json(req): Json<ActivateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let game_type = game_type_str
        .parse::<GameType>()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;

    // 根据游戏类型创建对应的适配器
    let result = match game_type {
        GameType::Cs2 => {
            let addr = req
                .listen_addr
                .unwrap_or_else(|| "127.0.0.1:3001".to_string());
            let mut adapter = crate::adapters::cs2::Cs2Adapter::new(addr);
            adapter.start_server().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError {
                        error: format!("Failed to start CS2 adapter: {}", e),
                    }),
                )
            })?;
            manager.activate(game_type, || Ok(Arc::new(adapter))).await
        }
        GameType::Lol => {
            let adapter = crate::adapters::lol::LolAdapter::new();
            manager.activate(game_type, || Ok(Arc::new(adapter))).await
        }
    };

    match result {
        Ok(()) => Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("{:?} adapter activated", game_type)
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("Failed to activate adapter: {}", e),
            }),
        )),
    }
}

/// POST /api/games/:game_type/deactivate - 关闭指定游戏的适配器
async fn deactivate_game(
    State((manager, _overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
    Path(game_type_str): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let game_type = game_type_str
        .parse::<GameType>()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;

    manager.deactivate(game_type).await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("{:?} adapter deactivated", game_type)
    })))
}

/// GET /api/games/:game_type/state - 获取指定游戏的当前状态
async fn get_game_state(
    State((manager, _overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
    Path(game_type_str): Path<String>,
) -> Result<Json<UnifiedGameState>, (StatusCode, Json<ApiError>)> {
    let game_type = game_type_str
        .parse::<GameType>()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;

    match manager.get_state(game_type).await {
        Ok(state) => Ok(Json(state)),
        Err(e) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiError {
                error: format!("Failed to get state: {}", e),
            }),
        )),
    }
}

/// GET /api/games/:game_type/status - 获取指定游戏适配器的状态
async fn get_game_status(
    State((manager, _overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
    Path(game_type_str): Path<String>,
) -> Result<Json<AdapterStatus>, (StatusCode, Json<ApiError>)> {
    let game_type = game_type_str
        .parse::<GameType>()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;

    match manager.get_status(game_type).await {
        Some(status) => Ok(Json(status)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "Adapter not active".to_string(),
            }),
        )),
    }
}

/// POST /api/games/{game_type}/auto-config - 一键自动配置
async fn auto_config_game(
    Path(game_type_str): Path<String>,
) -> Result<Json<AutoConfigResponse>, (StatusCode, Json<ApiError>)> {
    let game_type = game_type_str
        .parse::<GameType>()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;

    match game_type {
        GameType::Cs2 => {
            let result = crate::adapters::cs2::config_writer::auto_configure_cs2();
            Ok(Json(AutoConfigResponse {
                success: result.success,
                message: result.message,
                config_path: result.config_path.map(|p| p.to_string_lossy().to_string()),
            }))
        }
        GameType::Lol => Err((
            StatusCode::NOT_IMPLEMENTED,
            Json(ApiError {
                error: "LOL auto-config is not yet implemented".to_string(),
            }),
        )),
    }
}

/// GET /api/games/{game_type}/config-status - 检查是否已配置
async fn config_status(
    Path(game_type_str): Path<String>,
) -> Result<Json<ConfigStatusResponse>, (StatusCode, Json<ApiError>)> {
    let game_type = game_type_str
        .parse::<GameType>()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;

    match game_type {
        GameType::Cs2 => {
            let config_path = crate::adapters::cs2::config_writer::is_configured();
            Ok(Json(ConfigStatusResponse {
                is_configured: config_path.is_some(),
                config_path: config_path.map(|p| p.to_string_lossy().to_string()),
            }))
        }
        GameType::Lol => Ok(Json(ConfigStatusResponse {
            is_configured: false,
            config_path: None,
        })),
    }
}

/// GET /api/games/{game_type}/raw - 获取最后一次原始数据（调试用）
async fn get_raw_data(
    State((manager, _overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
    Path(game_type_str): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let game_type = game_type_str
        .parse::<GameType>()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;

    match manager.get_state(game_type).await {
        Ok(state) => Ok(Json(serde_json::json!({
            "timestamp": state.timestamp,
            "raw_payload": state.raw_payload,
            "parsed": {
                "game": state.game,
                "is_in_match": state.is_in_match,
                "map": state.map_or_arena,
                "mode": state.game_mode,
                "phase": state.match_phase,
                "local_player_name": state.local_player.as_ref().map(|p| &p.name),
                "local_player_stats": state.local_player.as_ref().map(|p| &p.stats),
            }
        }))),
        Err(e) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiError {
                error: format!("Failed to get raw data: {}", e),
            }),
        )),
    }
}

/// POST /api/games/:game_type/command - 执行适配器指令
/// 请求体：{ "cmd": "start_match" | "stop_match" | "list_queues" | "evaluate" | "send_chat" | "get_config" | "set_config", "args": {...} }
async fn game_command(
    State((manager, _overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
    Path(game_type_str): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let game_type = game_type_str
        .parse::<GameType>()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError { error: e })))?;

    let cmd = payload.get("cmd").and_then(|x| x.as_str()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "缺少 cmd 字段".to_string(),
            }),
        )
    })?;
    let args = payload
        .get("args")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    match manager.command(game_type, cmd, args).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )),
    }
}

/// GET /api/overlay/config - 获取 Overlay 配置
async fn get_overlay_config(
    State((_manager, overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
) -> Result<Json<OverlayConfig>, (StatusCode, Json<ApiError>)> {
    Ok(Json(overlay.get_config()))
}

/// POST /api/overlay/config - 更新 Overlay 配置
async fn update_overlay_config(
    State((_manager, overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
    Json(req): Json<UpdateOverlayRequest>,
) -> Result<Json<OverlayConfig>, (StatusCode, Json<ApiError>)> {
    let mut config = overlay.get_config();

    if let Some(enabled) = req.enabled {
        config.enabled = enabled;
    }
    if let Some(url) = req.url {
        config.url = url;
    }
    if let Some(player_steam_id) = req.player_steam_id {
        let player_steam_id = player_steam_id.trim().to_string();
        config.player_steam_id = if player_steam_id.is_empty() {
            None
        } else {
            Some(player_steam_id)
        };
    }
    if let Some(player_name) = req.player_name {
        if !player_name.is_empty() {
            config.player_name = player_name;
        }
    }
    overlay.update_config(config.clone());
    Ok(Json(config))
}

/// GET /api/overlay/debug - 获取 Overlay 死亡检测调试信息
async fn get_overlay_debug(
    State((_manager, overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
) -> Result<Json<crate::overlay::window::OverlayDebugSnapshot>, (StatusCode, Json<ApiError>)> {
    Ok(Json(overlay.get_debug()))
}

/// POST /api/overlay/show - 强制显示 Overlay 窗口
async fn show_overlay(
    State((_manager, overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    overlay.show();
    Ok(Json(
        serde_json::json!({"success": true, "message": "Overlay shown"}),
    ))
}

/// POST /api/overlay/hide - 强制隐藏 Overlay 窗口
async fn hide_overlay(
    State((_manager, overlay)): State<(Arc<AdapterManager>, OverlayCoordinator)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    overlay.hide();
    Ok(Json(
        serde_json::json!({"success": true, "message": "Overlay hidden"}),
    ))
}
