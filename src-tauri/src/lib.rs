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

pub use crate::api::cache::ApiCache;

pub struct AppState {
    pub paths: store::AppPaths,
    /// 进程级 TTL 响应缓存：UI 展示/只读命令共享；同步引擎用 fresh 实例（见 NeteaseApi）。
    pub api_cache: Arc<ApiCache>,
    pub sync_running: AtomicBool,
    pub cancel_requested: Arc<AtomicBool>,
    /// 暂停请求：同步任务在曲目边界检查该标志并等待（可继续/取消）。
    pub pause_requested: Arc<AtomicBool>,
    /// 云盘任务与歌单同步**并行运行**（资源不同：上传 vs 下载），拥有独立的运行/暂停/取消标志。
    pub cloud_running: AtomicBool,
    pub cloud_cancel_requested: Arc<AtomicBool>,
    pub cloud_pause_requested: Arc<AtomicBool>,
    /// 工具箱后台任务：NCM 转换、重复清理扫描、属性修复，各自独立运行。
    pub repair_task: crate::core::tool_task::ToolTaskState,
    pub ncm_task: crate::core::tool_task::ToolTaskState,
    pub cleanup_task: crate::core::tool_task::ToolTaskState,
}

pub fn run() {
    let paths = store::paths::DataPaths::discover()
        .expect("failed to initialize application data directory");
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // 单实例下启动时不可能有任务在跑：把上次退出残留的"进行中"日志
    // （应用被强杀/退出时任务没机会写结束状态）统一标记为已中断。
    {
        let app_paths = store::AppPaths::new(paths.clone());
        if let Ok(conn) = store::database::open(&app_paths.get().database_file) {
            let _ = store::database::finish_interrupted_logs(&conn);
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(AppState {
            paths: store::AppPaths::new(paths),
            api_cache: Arc::new(ApiCache::new()),
            sync_running: AtomicBool::new(false),
            cancel_requested: Arc::new(AtomicBool::new(false)),
            pause_requested: Arc::new(AtomicBool::new(false)),
            cloud_running: AtomicBool::new(false),
            cloud_cancel_requested: Arc::new(AtomicBool::new(false)),
            cloud_pause_requested: Arc::new(AtomicBool::new(false)),
            repair_task: crate::core::tool_task::ToolTaskState::new("repair"),
            ncm_task: crate::core::tool_task::ToolTaskState::new("ncm_convert"),
            cleanup_task: crate::core::tool_task::ToolTaskState::new("cleanup_scan"),
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
            commands::list_cloud_songs,
            commands::preview_cloud_upload,
            commands::sync_cloud_upload,
            commands::sync_cloud_manual,
            commands::download_cloud_songs,
            commands::cancel_cloud_sync,
            commands::pause_cloud_sync,
            commands::resume_cloud_sync,
            commands::get_cloud_control,
            commands::get_run_changes,
            commands::cancel_sync,
            commands::pause_sync,
            commands::resume_sync,
            commands::get_sync_control,
            commands::get_sync_logs,
            commands::list_quarantine,
            commands::restore_quarantine,
            commands::delete_quarantine,
            commands::manual_prune,
            commands::preflight_playlist,
            commands::preview_local_match,
            commands::preview_local_folder,
            commands::show_in_folder,
            commands::check_for_update,
            commands::get_sync_changes,
            commands::get_deleted_log,
            commands::get_playlist_history,
            commands::restore_deleted_item,
            commands::restore_playlist_snapshot_cmd,
            commands::get_account_stats,
            commands::get_local_stats,
            
            
            commands::cleanup_execute,
            commands::start_repair,
            commands::start_ncm_convert,
            commands::start_cleanup_scan,
            commands::get_tool_control,
            commands::cancel_tool,
            commands::pause_tool,
            commands::resume_tool,
            
            
            commands::quarantine_batch_restore,
            commands::quarantine_batch_delete,
            commands::set_auto_launch,
            commands::clear_sync_history_cmd,
            commands::preview_playlist_restore_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Music Auto Sync");
}
