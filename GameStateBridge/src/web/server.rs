use super::routes::api_routes;
use crate::core::manager::AdapterManager;
use crate::overlay::OverlayCoordinator;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use tracing::info;

/// 构建完整的 Web 应用路由
/// 包含 API 路由和静态文件服务
pub fn create_app(
    manager: Arc<AdapterManager>,
    overlay: OverlayCoordinator,
    static_dir: &str,
) -> Router {
    let api = api_routes(manager, overlay);

    Router::new()
        .merge(api)
        .fallback_service(ServeDir::new(static_dir))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

/// 启动 Web 服务器
pub async fn run_web_server(
    addr: SocketAddr,
    manager: Arc<AdapterManager>,
    overlay: OverlayCoordinator,
    static_dir: &str,
) -> anyhow::Result<()> {
    let app = create_app(manager, overlay, static_dir);

    info!("Web server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
