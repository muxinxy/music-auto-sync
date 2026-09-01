pub mod api;
pub mod commands;
pub mod core;
pub mod ncm;
pub mod runtime;
pub mod store;
pub mod tags;

use std::sync::atomic::AtomicBool;
use tauri::Manager;

pub struct AppState {
    pub paths: store::AppPaths,
    pub sync_running: AtomicBool,
    pub cancel_requested: AtomicBool,
}

pub fn run() {
    let paths = store::paths::DataPaths::discover()
        .expect("failed to initialize application data directory");
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(AppState {
            paths: store::AppPaths::new(paths),
            sync_running: AtomicBool::new(false),
            cancel_requested: AtomicBool::new(false),
        })
        .setup(|app| {
            runtime::tray::install(app.handle())?;
            runtime::scheduler::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_config,
            commands::save_config,
            commands::set_data_dir,
            commands::get_login_qr,
            commands::check_login_qr,
            commands::get_login_status,
            commands::open_login_log_directory,
            commands::logout,
            commands::list_playlists,
            commands::set_playlist_enabled,
            commands::sync_playlist,
            commands::sync_all,
            commands::cancel_sync,
            commands::get_sync_logs,
            commands::list_quarantine,
            commands::restore_quarantine,
            commands::delete_quarantine,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Music Auto Sync");
}
