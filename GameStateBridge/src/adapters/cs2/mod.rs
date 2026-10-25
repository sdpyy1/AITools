pub mod config_writer;
pub mod parser;
pub mod server;
pub mod types;

use crate::core::adapter::GameAdapter;
use crate::core::state::{AdapterStatus, GameType, UnifiedGameState};
use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

/// CS2 游戏适配器
pub struct Cs2Adapter {
    state: Arc<RwLock<server::GsiServerState>>,
    listen_addr: String,
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Cs2Adapter {
    pub fn new(listen_addr: impl Into<String>) -> Self {
        Self {
            state: Arc::new(RwLock::new(server::GsiServerState::default())),
            listen_addr: listen_addr.into(),
            server_handle: None,
        }
    }

    /// 启动 GSI HTTP Server（在后台运行）
    pub async fn start_server(&mut self) -> anyhow::Result<()> {
        let addr: SocketAddr = self
            .listen_addr
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid listen address: {}", e))?;

        let state = self.state.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = server::run_gsi_server(addr, state).await {
                error!("CS2 GSI server error: {}", e);
            }
        });

        self.server_handle = Some(handle);
        info!("CS2 adapter started, GSI server at http://{}/gsi/cs2", addr);
        Ok(())
    }
}

#[async_trait]
impl GameAdapter for Cs2Adapter {
    fn game_type(&self) -> GameType {
        GameType::Cs2
    }

    async fn current_state(&self) -> anyhow::Result<UnifiedGameState> {
        let guard = self.state.read().await;
        let payload = guard
            .latest_payload
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No CS2 GSI data received yet"))?;

        let state = parser::parse_cs2_state(payload);
        Ok(state)
    }

    fn status(&self) -> AdapterStatus {
        let is_connected = if let Ok(guard) = self.state.try_read() {
            guard.last_update.is_some()
        } else {
            false
        };

        let last_update = if let Ok(guard) = self.state.try_read() {
            guard.last_update
        } else {
            None
        };

        AdapterStatus {
            game_type: GameType::Cs2,
            is_initialized: self.server_handle.is_some(),
            is_connected,
            last_update,
            error_message: None,
        }
    }
}

impl Drop for Cs2Adapter {
    fn drop(&mut self) {
        if let Some(handle) = self.server_handle.take() {
            handle.abort();
            info!("CS2 adapter dropped, server task aborted");
        }
    }
}
