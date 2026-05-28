mod config;
mod patch;
mod server;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub use config::{AppConfig, ProfileStore, Settings};

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub config_path: PathBuf,
    pub log_state: Mutex<LogState>,
}

pub struct LogState {
    pub lines: Vec<String>,
    pub listeners: Vec<tokio::sync::broadcast::Sender<String>>,
    pub running: bool,
}

impl LogState {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            listeners: Vec::new(),
            running: false,
        }
    }
}

#[tokio::main]
async fn main() {
    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let config_path = exe_path.parent().unwrap_or(exe_path.as_path()).join("config.json");

    let profile_store = config::load_config(&config_path);
    let app_config = AppConfig { data: profile_store };

    let state = Arc::new(AppState {
        config: Mutex::new(app_config),
        config_path,
        log_state: Mutex::new(LogState::new()),
    });

    server::run_server(state).await;
}
