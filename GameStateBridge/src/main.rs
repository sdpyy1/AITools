use gamestate_bridge::adapters;
use gamestate_bridge::core::manager::AdapterManager;
use gamestate_bridge::core::registry::AdapterRegistry;
use gamestate_bridge::core::state::GameType;
use gamestate_bridge::overlay::OverlayCoordinator;
use gamestate_bridge::web::server::run_web_server;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter("gamestate_bridge=debug,tower_http=debug")
        .init();

    info!("Starting GameStateBridge...");

    // 创建适配器注册表
    let mut registry = AdapterRegistry::new();

    // 注册 CS2 适配器工厂
    registry.register(GameType::Cs2, || {
        Arc::new(adapters::cs2::Cs2Adapter::new("127.0.0.1:3001"))
    });

    // 注册 LOL 适配器工厂（占位）
    registry.register(GameType::Lol, || Arc::new(adapters::lol::LolAdapter::new()));

    // 创建管理器
    let manager = Arc::new(AdapterManager::new(registry));

    // 启动 Overlay 系统（死亡检测 + 浮窗）
    let overlay = OverlayCoordinator::start(manager.clone());

    // 启动 Web 服务器（同时提供 API 和静态页面）
    let web_addr: SocketAddr = "127.0.0.1:8080".parse()?;
    let static_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/web/static");

    info!("Open http://{} in your browser", web_addr);

    run_web_server(web_addr, manager, overlay, static_dir).await?;

    Ok(())
}
