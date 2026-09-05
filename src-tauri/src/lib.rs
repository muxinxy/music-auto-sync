pub mod api;
pub mod cli;
pub mod commands;
pub mod core;
pub mod error;
pub mod ncm;
pub mod runtime;
pub mod store;
pub mod tags;

use std::sync::{atomic::AtomicBool, Arc};
use tauri::Manager;

pub struct AppState {
    pub paths: store::AppPaths,
    pub sync_running: AtomicBool,
    pub cancel_requested: Arc<AtomicBool>,
    /// 暂停请求：同步任务在曲目边界检查该标志并等待（可继续/取消）。
    pub pause_requested: Arc<AtomicBool>,
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
            cancel_requested: Arc::new(AtomicBool::new(false)),
            pause_requested: Arc::new(AtomicBool::new(false)),
        })
        .setup(|app| {
            runtime::tray::install(app.handle())?;
            runtime::scheduler::start(app.handle().clone());

            // 关闭窗口时默认隐藏到托盘；可通过设置关闭。
            if let Some(window) = app.get_webview_window("main") {
                let win = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let state = win.state::<AppState>();
                        let close_to_tray = store::config::load(&state.paths.get().config_file)
                            .map(|config| config.close_to_tray)
                            .unwrap_or(true);
                        if close_to_tray {
                            api.prevent_close();
                            let _ = win.hide();
                        }
                    }
                });
            }
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
            commands::set_language,
            commands::logout,
            commands::send_login_captcha,
            commands::login_with_captcha,
            commands::list_playlists,
            commands::get_playlist_songs,
            commands::download_song_with_options,
            commands::set_playlist_enabled,
            commands::set_playlist_overwrite,
            commands::set_playlist_sync_policy,
            commands::get_playlist_settings,
            commands::sync_playlist,
            commands::sync_all,
            commands::cancel_sync,
            commands::pause_sync,
            commands::resume_sync,
            commands::get_sync_control,
            commands::get_sync_logs,
            commands::list_quarantine,
            commands::restore_quarantine,
            commands::delete_quarantine,
            commands::manual_prune,
            commands::get_liked_songs,
            commands::get_purchased_songs,
            commands::backup_songs,
            commands::preflight_playlist,
            commands::show_in_folder,
            commands::check_for_update,
            commands::get_sync_changes,
            commands::get_deleted_log,
            commands::get_playlist_history,
            commands::restore_deleted_item,
            commands::restore_playlist_snapshot_cmd,
            commands::get_account_stats,
            commands::get_local_stats,
            commands::convert_ncm_manual,
            commands::set_auto_launch,
            commands::clear_sync_history_cmd,
            commands::preview_playlist_restore_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Music Auto Sync");
}
