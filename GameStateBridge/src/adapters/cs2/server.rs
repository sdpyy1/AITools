use super::types::Cs2GameState;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// CS2 GSI HTTP Server 状态
#[derive(Debug, Clone, Default)]
pub struct GsiServerState {
    pub latest_payload: Option<Cs2GameState>,
    pub last_update: Option<chrono::DateTime<chrono::Utc>>,
    pub request_count: u64,
}

/// 启动 CS2 GSI 接收服务器
/// 返回 Router，由上层统一 serve
pub fn create_gsi_router(shared_state: Arc<RwLock<GsiServerState>>) -> Router {
    Router::new()
        .route("/gsi/cs2", post(gsi_handler))
        .with_state(shared_state)
}

/// GSI POST 请求处理器
async fn gsi_handler(
    State(state): State<Arc<RwLock<GsiServerState>>>,
    Json(payload): Json<Cs2GameState>,
) -> impl IntoResponse {
    debug!("Received CS2 GSI payload");

    let mut guard = state.write().await;
    guard.latest_payload = Some(payload);
    guard.last_update = Some(chrono::Utc::now());
    guard.request_count += 1;
    drop(guard);

    "OK"
}

/// 启动独立的 GSI HTTP Server（用于测试或单独运行）
pub async fn run_gsi_server(
    addr: SocketAddr,
    shared_state: Arc<RwLock<GsiServerState>>,
) -> anyhow::Result<()> {
    let app = create_gsi_router(shared_state);

    info!("CS2 GSI server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
