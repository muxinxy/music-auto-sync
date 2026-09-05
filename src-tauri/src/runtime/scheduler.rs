use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager};
use tokio::time::{sleep, Duration};

use crate::{core::sync, store, AppState};

pub fn start(app: AppHandle) {
    let state = app.state::<AppState>();
    let config = match store::config::load(&state.paths.get().config_file) {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(%error, "failed to load startup config");
            return;
        }
    };

    if config.auto_sync_on_startup && config.music_root.is_some() && config.cookie.is_some() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            sleep(Duration::from_secs(3)).await;
            let state = app.state::<AppState>();
            if !state.sync_running.load(Ordering::SeqCst) {
                if let Err(error) = sync::sync_enabled_with_source(&app, &state, "auto").await {
                    tracing::warn!(%error, "startup synchronization failed");
                }
            }
        });
    }

    if let Some(minutes) = config.sync_interval_minutes.filter(|x| *x >= 15) {
        tauri::async_runtime::spawn(async move {
            let interval = Duration::from_secs(minutes * 60);
            loop {
                sleep(interval).await;
                let state = app.state::<AppState>();
                if state.sync_running.load(Ordering::SeqCst) {
                    continue;
                }
                if let Err(error) = sync::sync_enabled_with_source(&app, &state, "scheduled").await {
                    tracing::warn!(%error, "scheduled synchronization failed");
                }
            }
        });
    }
}
