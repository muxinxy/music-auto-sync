use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{collections::HashMap, fs, path::Path, sync::atomic::Ordering};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    api::{
        ApiResponseMeta, LoginStatus, LoginStatusResponse, NeteaseApi, PlaylistInfo, QrCheckResult,
        QrSession, Track,
    },
    core::{
        naming,
        sync::{self, BatchItemResult, SyncReport},
    },
    error::UiMessage,
    store::{
        self,
        config::{Config, CookieUser, PlaylistSyncSetting},
        database,
        login_diagnostics::{self, LoginDiagnostic},
    },
    AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    data_dir: String,
    data_dir_portable: bool,
    version: String,
}

fn command_error(error: impl std::fmt::Display) -> String {
    UiMessage::unknown(error).to_json()
}

fn api_error_class(error: &anyhow::Error) -> String {
    error
        .downcast_ref::<crate::api::ApiCallError>()
        .map(|api_error| api_error.class.to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// 构建带进程级共享缓存的 API 客户端（仅 UI 展示 / 只读命令使用）。
/// 任何写路径、同步引擎路径一律用 `NeteaseApi::from_config`（fresh 空缓存），
/// 保证读取/写入都基于最新远端状态。
fn cached_api(state: &State<'_, AppState>, config: &Config) -> Result<NeteaseApi, String> {
    NeteaseApi::from_config_with_cache(config, state.api_cache.clone()).map_err(command_error)
}

fn anyhow_to_ui(error: anyhow::Error) -> String {
    if let Some(api_error) = error.downcast_ref::<crate::api::ApiCallError>() {
        api_error.ui().to_json()
    } else {
        UiMessage::unknown(error).to_json()
    }
}

#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> AppInfo {
    let paths = state.paths.get();
    AppInfo {
        data_dir: paths.root.to_string_lossy().into(),
        data_dir_portable: paths.portable,
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
    store::config::load(&state.paths.get().config_file).map_err(command_error)
}

#[tauri::command]
pub fn save_config(state: State<'_, AppState>, config: Config) -> Result<Config, String> {
    let paths = state.paths.get();
    store::config::save(&paths.config_file, &config).map_err(command_error)?;
    Ok(config)
}

#[tauri::command]
pub fn set_data_dir(
    state: State<'_, AppState>,
    dir: String,
    migrate: bool,
) -> Result<AppInfo, String> {
    let old = state.paths.get();
    let new = store::paths::DataPaths::from_root(dir.into(), true, old.exe_dir.clone())
        .map_err(command_error)?;
    if migrate && old.root != new.root {
        copy_dir_contents(&old.root, &new.root).map_err(command_error)?;
    }
    new.write_portable_marker(&new.root)
        .map_err(command_error)?;
    state.paths.set(new.clone());
    // 数据目录已切换：清空内存缓存（缓存 key 不跨目录复用）。
    state.api_cache.clear_all();
    Ok(AppInfo {
        data_dir: new.root.to_string_lossy().into(),
        data_dir_portable: true,
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

#[tauri::command]
pub async fn get_login_qr(state: State<'_, AppState>) -> Result<QrSession, String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    NeteaseApi::from_config(&config)
        .map_err(command_error)?
        .login_qr()
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn check_login_qr(
    state: State<'_, AppState>,
    key: String,
) -> Result<QrCheckResult, String> {
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file).map_err(command_error)?;
    let proxy_configured = proxy_is_configured(&config);
    let api = NeteaseApi::from_config(&config).map_err(command_error)?;
    let (result, cookie, qr_meta) = match api.check_qr(&key).await {
        Ok(result) => result,
        Err(error) => {
            log_login(
                &paths.logs_dir,
                diagnostic_from_meta(
                    "qr_check_failed",
                    "/login/qr/check",
                    "failed",
                    proxy_configured,
                    &error.meta,
                    Some(error.class),
                ),
            );
            return Err(error.ui().to_json());
        }
    };

    let qr_event = if result.state == "success" {
        "qr_authorized"
    } else {
        "qr_check"
    };
    let mut qr_diagnostic = diagnostic_from_meta(
        qr_event,
        "/login/qr/check",
        "received",
        proxy_configured,
        &qr_meta,
        None,
    );
    qr_diagnostic.qr_state = Some(result.state.clone());
    if result.state == "success" {
        qr_diagnostic.session_cookie_observed =
            Some(login_diagnostics::cookie_kind(cookie.as_deref()).into());
    }
    log_login(&paths.logs_dir, qr_diagnostic);

    if result.state == "success" {
        let cookie = cookie.ok_or_else(|| UiMessage::new("cookie_missing").to_json())?;
        let mut config = config;
        config.cookie = Some(cookie);
        config.cookie_user = None;
        store::config::save(&paths.config_file, &config).map_err(command_error)?;
        // 账号切换：清掉上个账号的缓存数据。
        state.api_cache.clear_all();

        let mut saved = LoginDiagnostic::new(
            "session_cookie_saved",
            "/login/qr/check",
            "saved",
            proxy_configured,
        );
        saved.cookie_persisted = Some(true);
        saved.session_cookie_observed =
            Some(login_diagnostics::cookie_kind(config.cookie.as_deref()).into());
        log_login(&paths.logs_dir, saved);

        match NeteaseApi::from_config(&config)
            .map_err(command_error)?
            .login_status()
            .await
        {
            Ok(response) => match login_verification(Some(&response.status)) {
                LoginVerification::Confirmed(user) => {
                    config.cookie_user = user;
                    store::config::save(&paths.config_file, &config).map_err(command_error)?;
                    log_login(
                        &paths.logs_dir,
                        status_diagnostic(
                            "status_confirmed",
                            "confirmed",
                            proxy_configured,
                            &response,
                            None,
                            None,
                            None,
                        ),
                    );
                }
                LoginVerification::Pending => {
                    tracing::warn!("QR login authorized but account profile is not available yet");
                    log_login(
                        &paths.logs_dir,
                        status_diagnostic(
                            "status_pending",
                            "pending",
                            proxy_configured,
                            &response,
                            Some("profile_pending"),
                            None,
                            None,
                        ),
                    );
                }
            },
            Err(error) => {
                tracing::warn!(%error, "QR login cookie saved but status verification is delayed");
                log_login(
                    &paths.logs_dir,
                    LoginDiagnostic {
                        error_class: Some(api_error_class(&error)),
                        ..LoginDiagnostic::new(
                            "status_failed",
                            "/login/status",
                            "failed",
                            proxy_configured,
                        )
                    },
                );
            }
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn get_login_status(
    state: State<'_, AppState>,
    verify_attempt: Option<u8>,
    retry_limit: Option<u8>,
) -> Result<LoginStatus, String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    let proxy_configured = proxy_is_configured(&config);
    let status_response = match NeteaseApi::from_config(&config)
        .map_err(command_error)?
        .login_status()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            log_login(
                &paths.logs_dir,
                LoginDiagnostic {
                    verify_attempt,
                    retry_limit,
                    error_class: Some(api_error_class(&error)),
                    ..LoginDiagnostic::new(
                        "status_failed",
                        "/login/status",
                        "failed",
                        proxy_configured,
                    )
                },
            );
            return Err(anyhow_to_ui(error));
        }
    };
    let status = status_response.status.clone();

    if status.logged_in {
        let next_user = cookie_user_from_status(&status);
        if config.cookie_user != next_user {
            config.cookie_user = next_user;
            store::config::save(&paths.config_file, &config).map_err(command_error)?;
        }
        log_login(
            &paths.logs_dir,
            status_diagnostic(
                "status_confirmed",
                "confirmed",
                proxy_configured,
                &status_response,
                None,
                verify_attempt,
                retry_limit,
            ),
        );
    } else if config.cookie.is_some() {
        log_login(
            &paths.logs_dir,
            status_diagnostic(
                "status_pending",
                "pending",
                proxy_configured,
                &status_response,
                Some("profile_pending"),
                verify_attempt,
                retry_limit,
            ),
        );
    }
    Ok(status)
}

#[tauri::command]
pub fn logout(state: State<'_, AppState>) -> Result<(), String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    config.cookie = None;
    config.cookie_user = None;
    store::config::save(&paths.config_file, &config).map_err(command_error)?;
    // 账号已切换：清空所有按账号/歌单缓存，避免泄露上一个账号的数据。
    state.api_cache.clear_all();
    Ok(())
}

#[tauri::command]
pub async fn list_playlists(
    state: State<'_, AppState>,
    force: Option<bool>,
) -> Result<Vec<PlaylistInfo>, String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    let user = config
        .cookie_user
        .as_ref()
        .context("请先登录")
        .map_err(command_error)?;
    let api = cached_api(&state, &config)?;
    // force=true（UI 刷新按钮）：穿透 5 分钟缓存直拉网易并回填。
    let mut playlists = if force.unwrap_or(false) {
        api.user_playlists_forced(user.user_id, &config)
            .await
            .map_err(command_error)?
    } else {
        api.user_playlists(user.user_id, &config)
            .await
            .map_err(command_error)?
    };
    fill_synced_counts(&state, &mut playlists);
    fill_last_sync(&state, &mut playlists);
    Ok(playlists)
}

fn fill_last_sync(state: &State<'_, AppState>, playlists: &mut [PlaylistInfo]) {
    let Ok(conn) = database::open(&state.paths.get().database_file) else {
        return;
    };
    let Ok(rows) = (|| -> Result<Vec<(u64, String, i64)>> {
        let mut stmt = conn
            .prepare("SELECT playlist_id, finished_at, failed FROM sync_runs ORDER BY id DESC")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })() else {
        return;
    };
    let mut latest: HashMap<u64, (String, i64)> = HashMap::new();
    for (playlist_id, finished_at, failed) in rows {
        latest.entry(playlist_id).or_insert((finished_at, failed));
    }
    for playlist in playlists.iter_mut() {
        if let Some((finished_at, failed)) = latest.get(&playlist.id) {
            playlist.last_sync = Some(finished_at.clone());
            playlist.last_result = if *failed == 0 {
                Some(UiMessage::new("sync_ok").to_json())
            } else {
                Some(UiMessage::with_params("sync_done_failed", vec![failed.to_string()]).to_json())
            };
        }
    }
}

fn fill_synced_counts(state: &State<'_, AppState>, playlists: &mut [PlaylistInfo]) {
    let Ok(conn) = database::open(&state.paths.get().database_file) else {
        return;
    };
    let Ok(rows) = (|| -> Result<Vec<(u64, String)>> {
        let mut stmt = conn.prepare("SELECT playlist_id, local_path FROM track_files")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })() else {
        return;
    };
    let mut counts: HashMap<u64, u32> = HashMap::new();
    for (playlist_id, local_path) in rows {
        if Path::new(&local_path).is_file() {
            *counts.entry(playlist_id).or_default() += 1;
        }
    }
    for playlist in playlists.iter_mut() {
        playlist.synced = counts.get(&playlist.id).copied().unwrap_or(0);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSong {
    pub id: u64,
    pub name: String,
    pub artists: String,
    pub album: String,
    pub duration_ms: u64,
    pub position: usize,
    pub local_path: Option<String>,
    pub synced: bool,
    pub file_size: Option<u64>,
    pub file_modified: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSongsResult {
    pub playlist_id: u64,
    pub playlist_name: String,
    pub songs: Vec<PlaylistSong>,
}

#[tauri::command]
pub async fn get_playlist_songs(
    state: State<'_, AppState>,
    id: u64,
    force: Option<bool>,
) -> Result<PlaylistSongsResult, String> {
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file).map_err(command_error)?;
    // UI 展示：读进程级共享缓存，避免重复整表拉取（歌单变更后由同步/下载完成事件
    // 与 TTL 双重保证新鲜度）。force=true（抽屉“刷新”按钮）穿透缓存直拉。
    let api = cached_api(&state, &config)?;
    let playlist = if force.unwrap_or(false) {
        api.playlist_tracks_forced(id).await
    } else {
        api.playlist_tracks(id).await
    }
    .map_err(command_error)?;
    let conn = database::open(&paths.database_file).map_err(command_error)?;
    let mut songs = Vec::with_capacity(playlist.tracks.len());
    for (index, track) in playlist.tracks.iter().enumerate() {
        let local_path: Option<String> = conn
            .query_row(
                "SELECT local_path FROM track_files WHERE playlist_id=?1 AND track_id=?2",
                rusqlite::params![playlist.id, track.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(command_error)?;
        let exists = local_path
            .as_deref()
            .is_some_and(|path| Path::new(path).is_file());
        let file_size = local_path
            .as_deref()
            .and_then(|path| fs::metadata(path).ok())
            .map(|meta| meta.len());
        let file_modified = local_path
            .as_deref()
            .and_then(|path| fs::metadata(path).ok())
            .and_then(|meta| meta.modified().ok())
            .map(|time| {
                let datetime: chrono::DateTime<chrono::Local> = time.into();
                datetime.format("%Y-%m-%d %H:%M:%S").to_string()
            });
        songs.push(PlaylistSong {
            id: track.id,
            name: track.name.clone(),
            artists: naming::artists_with(track, &config.artist_separator),
            album: track.al.name.clone(),
            duration_ms: track.dt,
            position: index + 1,
            synced: exists,
            local_path,
            file_size,
            file_modified,
        });
    }
    Ok(PlaylistSongsResult {
        playlist_id: playlist.id,
        playlist_name: playlist.name,
        songs,
    })
}

#[tauri::command]
pub async fn download_song_with_options(
    app: AppHandle,
    state: State<'_, AppState>,
    playlist_id: u64,
    track_id: u64,
    options: sync::SingleDownloadOptions,
) -> Result<String, String> {
    sync::download_song_with_options(Some(&app), &state, playlist_id, track_id, options)
        .await
        .map_err(|message| message.to_json())
}

#[tauri::command]
pub fn set_playlist_overwrite(
    state: State<'_, AppState>,
    id: u64,
    overwrite: bool,
) -> Result<(), String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    if let Some(setting) = config.playlists.iter_mut().find(|p| p.id == id) {
        setting.overwrite = overwrite;
    } else {
        config.playlists.push(PlaylistSyncSetting {
            id,
            name: format!("歌单 {id}"),
            enabled: false,
            folder_override: None,
            quality_override: None,
            overwrite,
            mode_override: None,
            upload_manual: None,
        });
    }
    store::config::save(&paths.config_file, &config).map_err(command_error)
}

#[tauri::command]
pub fn set_playlist_enabled(
    state: State<'_, AppState>,
    id: u64,
    enabled: bool,
) -> Result<(), String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    if let Some(setting) = config.playlists.iter_mut().find(|p| p.id == id) {
        setting.enabled = enabled;
    } else {
        config.playlists.push(PlaylistSyncSetting {
            id,
            name: format!("歌单 {id}"),
            enabled,
            folder_override: None,
            quality_override: None,
            overwrite: false,
            mode_override: None,
            upload_manual: None,
        });
    }
    store::config::save(&paths.config_file, &config).map_err(command_error)
}

/// 返回歌单当前的同步策略（覆盖值 + 全局默认，供详情面板初始化与展示）。
#[tauri::command]
pub fn get_playlist_settings(state: State<'_, AppState>, id: u64) -> Result<serde_json::Value, String> {
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file).map_err(command_error)?;
    let setting = config.playlists.iter().find(|p| p.id == id);
    Ok(serde_json::json!({
        "playlistId": id,
        "modeOverride": setting.and_then(|s| s.mode_override.clone()),
        "uploadManual": setting.and_then(|s| s.upload_manual),
        "globalMode": config.sync_mode,
        "globalUploadManual": config.upload_manual,
    }))
}
/// 设置单个歌单的同步策略：模式覆盖（None/空字符串 = 跟随全局默认）与
/// “补录手动放入的歌”开关覆盖（None = 跟随全局默认）。
#[tauri::command]
pub fn set_playlist_sync_policy(
    state: State<'_, AppState>,
    id: u64,
    mode: Option<String>,
    upload_manual: Option<bool>,
) -> Result<(), String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    let mode = mode
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    if let Some(setting) = config.playlists.iter_mut().find(|p| p.id == id) {
        setting.mode_override = mode;
        setting.upload_manual = upload_manual;
    } else {
        config.playlists.push(PlaylistSyncSetting {
            id,
            name: format!("歌单 {id}"),
            enabled: false,
            folder_override: None,
            quality_override: None,
            overwrite: false,
            mode_override: mode,
            upload_manual,
        });
    }
    store::config::save(&paths.config_file, &config).map_err(command_error)
}

#[tauri::command]
pub async fn sync_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    id: u64,
) -> Result<SyncReport, String> {
    let paths = state.paths.get();
    let _ = store::app_log::log(
        &paths.logs_dir,
        "info",
        "sync",
        format!("开始同步歌单 {id}"),
    );
    sync::sync_one(&app, &state, id)
        .await
        .map(|report| {
            let _ = store::app_log::log(
                &paths.logs_dir,
                "info",
                "sync",
                format!(
                    "歌单 {} 同步完成：新增 {} 失败 {}",
                    id, report.added, report.failed
                ),
            );
            report
        })
        .map_err(|message| {
            let _ = store::app_log::log(
                &paths.logs_dir,
                "error",
                "sync",
                format!("歌单 {id} 同步失败：{message:?}"),
            );
            message.to_json()
        })
}

#[tauri::command]
pub async fn sync_all(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SyncReport>, String> {
    let paths = state.paths.get();
    let _ = store::app_log::log(&paths.logs_dir, "info", "sync", "开始同步全部歌单");
    sync::sync_enabled(&app, &state).await.map_err(|message| {
        let _ = store::app_log::log(
            &paths.logs_dir,
            "error",
            "sync",
            format!("同步全部失败：{message:?}"),
        );
        message.to_json()
    })
}

#[tauri::command]
pub fn cancel_sync(state: State<'_, AppState>) -> bool {
    if state.sync_running.load(Ordering::SeqCst) {
        state.cancel_requested.store(true, Ordering::SeqCst);
        state.pause_requested.store(false, Ordering::SeqCst);
        true
    } else {
        false
    }
}

/// 暂停当前同步任务（在曲目/歌单边界生效）。
#[tauri::command]
pub fn pause_sync(state: State<'_, AppState>) -> bool {
    if state.sync_running.load(Ordering::SeqCst) {
        state.pause_requested.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
}

/// 继续被暂停的同步任务。
#[tauri::command]
pub fn resume_sync(state: State<'_, AppState>) -> bool {
    if state.sync_running.load(Ordering::SeqCst) && state.pause_requested.swap(false, Ordering::SeqCst)
    {
        true
    } else {
        false
    }
}

/// 查询同步控制状态：running / paused。
#[tauri::command]
pub fn get_sync_control(state: State<'_, AppState>) -> serde_json::Value {
    serde_json::json!({
        "running": state.sync_running.load(Ordering::SeqCst),
        "paused": state.pause_requested.load(Ordering::SeqCst),
    })
}

#[tauri::command]
pub fn get_sync_logs(
    state: State<'_, AppState>,
    limit: usize,
) -> Result<Vec<database::SyncLogEntry>, String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    database::get_logs(&conn, limit.min(500)).map_err(command_error)
}

#[tauri::command]
pub fn list_quarantine(
    state: State<'_, AppState>,
) -> Result<Vec<database::QuarantineItem>, String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    database::list_quarantine(&conn).map_err(command_error)
}

#[tauri::command]
pub fn restore_quarantine(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    let item: (String, String) = conn
        .query_row(
            "SELECT original_path, quarantine_path FROM quarantine WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(command_error)?;
    let original = Path::new(&item.0);
    let quarantined = Path::new(&item.1);
    if !quarantined.is_file() {
        return Err(UiMessage::new("quarantine_missing").to_json());
    }
    if original.exists() {
        return Err(UiMessage::new("quarantine_conflict").to_json());
    }
    if let Some(parent) = original.parent() {
        fs::create_dir_all(parent).map_err(command_error)?;
    }
    fs::rename(quarantined, original).map_err(command_error)?;
    conn.execute("DELETE FROM quarantine WHERE id=?1", [id])
        .map_err(command_error)?;
    Ok(())
}

#[tauri::command]
pub fn delete_quarantine(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    let path: Option<String> = conn
        .query_row(
            "SELECT quarantine_path FROM quarantine WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(command_error)?;
    let path = path.ok_or_else(|| UiMessage::new("quarantine_record_missing").to_json())?;
    if Path::new(&path).is_file() {
        fs::remove_file(&path).map_err(command_error)?;
    }
    conn.execute("DELETE FROM quarantine WHERE id=?1", [id])
        .map_err(command_error)?;
    Ok(())
}

#[tauri::command]
pub fn open_login_log_directory(state: State<'_, AppState>) -> Result<(), String> {
    let paths = state.paths.get();
    login_diagnostics::log_dir(&paths.logs_dir).map_err(command_error)?;
    std::process::Command::new("explorer")
        .arg(&paths.logs_dir)
        .spawn()
        .map_err(|_| UiMessage::new("log_open_failed").to_json())?;
    Ok(())
}

#[tauri::command]
pub fn set_language(
    app: AppHandle,
    state: State<'_, AppState>,
    language: String,
) -> Result<(), String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    let language: String = if language.starts_with("en") {
        "en".into()
    } else {
        "zh-CN".into()
    };
    if config.language != language {
        config.language = language.clone();
        store::config::save(&paths.config_file, &config).map_err(command_error)?;
    }
    crate::runtime::tray::install(&app).map_err(command_error)?;
    let title = if language == "en" {
        "Music Auto Sync"
    } else {
        "音乐同步"
    };
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(title);
    }
    Ok(())
}

fn proxy_is_configured(config: &Config) -> bool {
    config
        .http_proxy
        .as_deref()
        .is_some_and(|proxy| !proxy.trim().is_empty())
}

fn log_login(logs_dir: &Path, entry: LoginDiagnostic) {
    if let Err(error) = login_diagnostics::append(logs_dir, entry) {
        tracing::warn!(%error, "failed to write login diagnostic");
    }
}

fn diagnostic_from_meta(
    event: &str,
    endpoint: &str,
    outcome: &str,
    proxy_configured: bool,
    meta: &ApiResponseMeta,
    error_class: Option<&str>,
) -> LoginDiagnostic {
    LoginDiagnostic {
        duration_ms: Some(meta.duration_ms),
        http_status: meta.http_status,
        api_code: meta.api_code,
        server: meta.server.clone(),
        request_id: meta.request_id.clone(),
        retry_after: meta.retry_after.clone(),
        error_class: error_class.map(str::to_owned),
        ..LoginDiagnostic::new(event, endpoint, outcome, proxy_configured)
    }
}

fn status_diagnostic(
    event: &str,
    outcome: &str,
    proxy_configured: bool,
    response: &LoginStatusResponse,
    error_class: Option<&str>,
    verify_attempt: Option<u8>,
    retry_limit: Option<u8>,
) -> LoginDiagnostic {
    LoginDiagnostic {
        http_status: response.meta.http_status,
        api_code: response.meta.api_code,
        profile_present: Some(
            response.status.user_id.is_some() || response.status.nickname.is_some(),
        ),
        account_present: Some(response.account_present),
        verify_attempt,
        retry_limit,
        error_class: error_class.map(str::to_owned),
        ..LoginDiagnostic::new(event, "/login/status", outcome, proxy_configured)
    }
}

fn cookie_user_from_status(status: &LoginStatus) -> Option<CookieUser> {
    match (status.user_id, status.nickname.clone()) {
        (Some(user_id), Some(nickname)) => Some(CookieUser { user_id, nickname }),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LoginVerification {
    Confirmed(Option<CookieUser>),
    Pending,
}

fn login_verification(status: Option<&LoginStatus>) -> LoginVerification {
    match status {
        Some(status) if status.logged_in => {
            LoginVerification::Confirmed(cookie_user_from_status(status))
        }
        _ => LoginVerification::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_authorized_session_pending_when_profile_is_empty() {
        let status = LoginStatus {
            logged_in: false,
            nickname: None,
            user_id: None,
            avatar_url: None,
        };
        assert_eq!(
            login_verification(Some(&status)),
            LoginVerification::Pending
        );
    }

    #[test]
    fn keeps_authorized_session_pending_when_status_is_unavailable() {
        assert_eq!(login_verification(None), LoginVerification::Pending);
    }

    #[test]
    fn confirms_session_when_profile_is_available() {
        let status = LoginStatus {
            logged_in: true,
            nickname: Some("测试用户".into()),
            user_id: Some(42),
            avatar_url: None,
        };
        assert_eq!(
            login_verification(Some(&status)),
            LoginVerification::Confirmed(Some(CookieUser {
                user_id: 42,
                nickname: "测试用户".into(),
            }))
        );
    }
}

fn copy_dir_contents(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_dir_contents(&source, &target)?;
        } else if !target.exists() {
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// 发送手机验证码（短信登录第一步）。
#[tauri::command]
pub async fn send_login_captcha(state: State<'_, AppState>, phone: String) -> Result<(), String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    let phone = phone.trim().to_owned();
    if phone.is_empty() {
        return Err(UiMessage::new("phone_required").to_json());
    }
    NeteaseApi::from_config(&config)
        .map_err(command_error)?
        .send_captcha(&phone, "86")
        .await
        .map_err(command_error)
}

/// 用验证码登录并保存会话。
#[tauri::command]
pub async fn login_with_captcha(
    state: State<'_, AppState>,
    phone: String,
    captcha: String,
) -> Result<LoginStatus, String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    let phone = phone.trim().to_owned();
    let captcha = captcha.trim().to_owned();
    if phone.is_empty() || captcha.is_empty() {
        return Err(UiMessage::new("phone_or_code_required").to_json());
    }
    let (cookie, status) = NeteaseApi::from_config(&config)
        .map_err(command_error)?
        .login_cellphone(&phone, &captcha)
        .await
        .map_err(command_error)?;
    config.cookie = Some(cookie);
    config.cookie_user = status.as_ref().and_then(|s| cookie_user_from_status(s));
    store::config::save(&paths.config_file, &config).map_err(command_error)?;
    // 账号切换：清掉上个账号的缓存数据。
    state.api_cache.clear_all();
    Ok(status.unwrap_or(LoginStatus {
        logged_in: true,
        nickname: None,
        user_id: None,
        avatar_url: None,
    }))
}

/// 获取“我喜欢”的歌曲详情列表。
#[tauri::command]
pub async fn get_liked_songs(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    let user = config
        .cookie_user
        .as_ref()
        .context("请先登录")
        .map_err(command_error)?;
    let api = NeteaseApi::from_config(&config).map_err(command_error)?;
    let ids = api
        .liked_song_ids(user.user_id)
        .await
        .map_err(command_error)?;
    let details = api.song_detail_batch(&ids).await.map_err(command_error)?;
    let mut out: Vec<serde_json::Value> = details.into_values().collect();
    out.sort_by_key(|v| v.get("id").and_then(|x| x.as_u64()).unwrap_or(0));
    Ok(out)
}

/// 获取已购单曲详情列表。
#[tauri::command]
pub async fn get_purchased_songs(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    let api = NeteaseApi::from_config(&config).map_err(command_error)?;
    let ids = api.purchased_songs().await.map_err(command_error)?;
    let details = api.song_detail_batch(&ids).await.map_err(command_error)?;
    let mut out: Vec<serde_json::Value> = details.into_values().collect();
    out.sort_by_key(|v| v.get("id").and_then(|x| x.as_u64()).unwrap_or(0));
    Ok(out)
}

/// 备份“我喜欢 / 已购”到指定目录（不纳入任何歌单的已同步状态）。
#[tauri::command]
pub async fn backup_songs(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    label: String,
    target_dir: String,
    quality: Option<String>,
    write_lrc: Option<bool>,
    overwrite: bool,
) -> Result<Vec<BatchItemResult>, String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    let api = NeteaseApi::from_config(&config).map_err(command_error)?;
    let ids: Vec<u64> = match kind.as_str() {
        "liked" => {
            let user = config
                .cookie_user
                .as_ref()
                .context("请先登录")
                .map_err(command_error)?;
            api.liked_song_ids(user.user_id)
                .await
                .map_err(command_error)?
        }
        "purchased" => api.purchased_songs().await.map_err(command_error)?,
        other => {
            return Err(UiMessage::with_params("invalid_kind", vec![other.to_owned()]).to_json())
        }
    };
    if ids.is_empty() {
        return Err(UiMessage::new("no_songs_to_backup").to_json());
    }
    // 批量详情一次拿全（避免逐首 /playlist/track/all）。
    let details = api.song_detail_batch(&ids).await.map_err(command_error)?;
    let tracks: Vec<Track> = ids
        .iter()
        .filter_map(|id| details.get(id))
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();
    let dir = Path::new(&target_dir);
    sync::download_track_ids(
        Some(&app),
        &state,
        dir,
        &label,
        quality.as_deref(),
        write_lrc,
        overwrite,
        tracks,
    )
    .await
    .map_err(|m| m.to_json())
}

/// 手动把“不在歌单里的本地文件”隔离进 .quarantine。
#[tauri::command]
pub async fn manual_prune(state: State<'_, AppState>, id: u64) -> Result<usize, String> {
    if state.sync_running.load(Ordering::SeqCst) {
        return Err(UiMessage::new("sync_busy").to_json());
    }
    sync::prune_playlist_removed(&state, id)
        .await
        .map_err(|message| message.to_json())
}

/// 本地文件匹配预览条目：歌单文件夹里每个音频的解析结果。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMatchPreview {
    /// 本地文件路径（歌单文件夹内）。
    pub path: String,
    /// 文件名（不含目录）。
    pub file_name: String,
    /// 解析出的网易曲目 id（旁车/ID3/文件名匹配；无命中则为 null）。
    pub netease_id: Option<u64>,
    /// 是否命中当前歌单中的曲目。
    pub matched: bool,
    /// 命中时对应的曲目名。
    pub track_name: Option<String>,
    /// 该曲目是否已在歌单标记为已同步（DB 登记且文件存在）。
    pub synced: bool,
    /// 该本地文件是否正是 DB 中登记的已同步文件（路径一致）。
    pub is_registered_file: bool,
    /// 匹配来源：sidecar / key163 / id3 / tag / none。
    pub match_kind: String,
}

/// 扫描一个本地歌单文件夹并把其中音频与给定曲目列表匹配（共用核心）。
/// 优先级：旁车 → 标签 163 key/netease-id → ID3 标签标题+艺术家。纯只读，零网络。
async fn preview_folder_matches(
    folder: std::path::PathBuf,
    playlist_id: u64,
    tracks: &[crate::api::Track],
    db_file: &std::path::Path,
) -> Vec<LocalMatchPreview> {
    if !folder.is_dir() {
        return Vec::new();
    }
    let canonical_folder = folder.canonicalize().unwrap_or(folder.clone());
    // DB 已登记路径（判定 synced / is_registered_file）。
    let registered_map: std::collections::HashMap<u64, String> = database::open(db_file)
        .ok()
        .and_then(|conn| {
            let mut stmt = conn
                .prepare("SELECT track_id, local_path FROM track_files WHERE playlist_id=?1")
                .ok()?;
            let rows = stmt
                .query_map([playlist_id as i64], |row| {
                    Ok((row.get::<_, i64>(0)? as u64, row.get(1)?))
                })
                .ok()?
                .collect::<std::result::Result<Vec<_>, _>>()
                .ok()?;
            Some(rows.into_iter().collect())
        })
        .unwrap_or_default();

    // 文件名匹配候选：尚未被可靠识别占用的曲目，按歌名预筛以提速。
    let mut used_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (path, marker_id) in sync::list_local_audio_with_id(&folder) {
        let file_name = path
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or_default()
            .to_owned();
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        let is_registered_file = registered_map.values().any(|p| {
            std::path::Path::new(p)
                .canonicalize()
                .map(|c| c == canonical)
                .unwrap_or(false)
        });

        // 1) 旁车/标签可靠标记。细分来源：旁车 → sidecar；
        //    标签里的 163 key 解出的 → key163；旧版纯文本 netease-id → id3。
        let mut netease_id = marker_id;
        let sidecar_exists = path
            .with_extension(format!(
                "{}.netease.json",
                path.extension().and_then(|x| x.to_str()).unwrap_or("mp3")
            ))
            .is_file();
        let mut match_kind = if sidecar_exists {
            Some("sidecar")
        } else if marker_id.is_some() {
            // 区分 163 key 与旧纯文本：读 comment 文本判断。
            let has_163 = crate::core::sync::read_comment_text(&path)
                .is_some_and(|t| t.contains(crate::core::netease_key::KEY_PREFIX));
            Some(if has_163 { "key163" } else { "id3" })
        } else {
            None
        };
        // 2) ID3 标签标题+艺术家匹配（仅当标记缺失且非已登记文件时兜底）。
        if netease_id.is_none() && !is_registered_file {
            if let Some((tag_title, tag_artist)) = crate::core::sync::read_local_tags(&path) {
                if let Some(t) = tracks.iter().find(|t| {
                    !used_ids.contains(&t.id)
                        && crate::core::filename_match::tag_matches_track(
                            &tag_title,
                            &tag_artist,
                            t,
                        )
                }) {
                    netease_id = Some(t.id);
                    match_kind = Some("tag");
                }
            }
        }

        let in_playlist = netease_id.is_some_and(|id| tracks.iter().any(|t| t.id == id));
        if let Some(id) = netease_id {
            used_ids.insert(id);
        }
        let track_name = netease_id
            .and_then(|id| tracks.iter().find(|t| t.id == id).map(|t| t.name.clone()));
        let synced = in_playlist
            && netease_id.is_some_and(|id| {
                registered_map
                    .get(&id)
                    .is_some_and(|p| std::path::Path::new(p).is_file())
            });
        let display_path = canonical
            .strip_prefix(&canonical_folder)
            .unwrap_or(&canonical)
            .to_string_lossy()
            .into_owned();
        out.push(LocalMatchPreview {
            path: display_path,
            file_name,
            netease_id,
            matched: in_playlist,
            track_name,
            synced,
            is_registered_file,
            match_kind: match_kind.unwrap_or("none").to_owned(),
        });
    }
    out
}

/// 本地匹配预览（按网易歌单 id，只读）：扫描该歌单的本地文件夹，展示每个本地音频
/// 解析出的网易曲目、是否在歌单、是否已同步。不下载、不改名、不写库。
#[tauri::command]
pub async fn preview_local_match(
    state: State<'_, AppState>,
    id: u64,
) -> Result<Vec<LocalMatchPreview>, String> {
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file).map_err(command_error)?;
    let root = config
        .music_root
        .as_deref()
        .map(std::path::PathBuf::from)
        .context("请先在设置中配置音乐根目录")
        .map_err(command_error)?;
    // 预览需要最新曲目：穿透缓存强制拉取。
    let api = cached_api(&state, &config)?;
    let playlist = api.playlist_tracks_forced(id).await.map_err(command_error)?;
    let folder = sync::playlist_folder_path(
        &root,
        &config.folder_template,
        &config.artist_separator,
        &playlist,
    );
    Ok(preview_folder_matches(folder, id, &playlist.tracks, &paths.database_file).await)
}

/// 本地匹配预览（按本地文件夹名驱动）：musicRoot 下名为 `folder` 的子目录即歌单文件夹。
/// 若网易账号存在同名歌单则用其曲目列表匹配；找不到同名歌单时仅列出本地文件（未匹配）。
#[tauri::command]
pub async fn preview_local_folder(
    state: State<'_, AppState>,
    folder: String,
) -> Result<Vec<LocalMatchPreview>, String> {
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file).map_err(command_error)?;
    let root = config
        .music_root
        .as_deref()
        .map(std::path::PathBuf::from)
        .context("请先在设置中配置音乐根目录")
        .map_err(command_error)?;
    let folder_path = root.join(&folder);
    if !folder_path.is_dir() {
        return Ok(vec![]);
    }
    let api = cached_api(&state, &config)?;
    // 找网易同名歌单 id（归一化比较，兼容大小写/空格）。
    let user = config
        .cookie_user
        .as_ref()
        .context("请先登录")
        .map_err(command_error)?;
    let mut playlist_id: u64 = 0;
    let mut tracks: Vec<crate::api::Track> = Vec::new();
    if let Ok(playlists) = api.user_playlists(user.user_id, &config).await {
        let norm = |s: &str| crate::core::filename_match::normalize_title(s);
        let target = norm(&folder);
        if let Some(matched) = playlists.iter().find(|p| norm(&p.name) == target) {
            if let Ok(pl) = api.playlist_tracks_forced(matched.id).await {
                playlist_id = matched.id;
                tracks = pl.tracks;
            }
        }
    }
    if playlist_id == 0 {
        // 无同名歌单：用 0 作为“无对照”，仅列文件。
        playlist_id = 0;
    }
    Ok(preview_folder_matches(folder_path, playlist_id, &tracks, &paths.database_file).await)
}

/// 预检歌单歌曲可用性与最高音质（供歌曲列表标注）。
/// 曲目列表走共享缓存去重；预检结果本身（VIP/版权实时）从不缓存。
/// force=true（抽屉“刷新”按钮）穿透曲目列表缓存直拉。
#[tauri::command]
pub async fn preflight_playlist(
    state: State<'_, AppState>,
    id: u64,
    force: Option<bool>,
) -> Result<Vec<serde_json::Value>, String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    let api = cached_api(&state, &config)?;
    let playlist = if force.unwrap_or(false) {
        api.playlist_tracks_forced(id).await
    } else {
        api.playlist_tracks(id).await
    }
    .map_err(command_error)?;
    let preflight = api
        .preflight_tracks(&playlist.tracks)
        .await
        .map_err(command_error)?;
    Ok(preflight
        .into_iter()
        .map(|entry| serde_json::to_value(entry).unwrap_or(serde_json::json!({})))
        .collect())
}

/// 在系统文件管理器中显示某个文件/目录。
#[tauri::command]
pub fn show_in_folder(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(UiMessage::with_params("path_missing", vec![path]).to_json());
    }
    let _ = state;
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("explorer");
        cmd.arg("/select,");
        if p.is_dir() {
            // explorer /select, 对目录会退化为打开其父目录；直接打开该目录更符合预期。
            cmd = std::process::Command::new("explorer");
            cmd.arg(&path);
        } else {
            cmd.arg(&path);
        }
        cmd.spawn()
            .map_err(|_| UiMessage::new("explorer_failed").to_json())?;
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        return Err(UiMessage::new("unsupported_platform").to_json());
    }
}

/// 检查 GitHub Releases 是否有新版本（静默失败）。
#[tauri::command]
pub async fn check_for_update(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let _ = state;
    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(command_error)?;
    let url = "https://api.github.com/repos/muxinxy/music-auto-sync/releases/latest";
    let response = client
        .get(url)
        .header("User-Agent", "MusicAutoSync")
        .send()
        .await
        .map_err(|error| {
            UiMessage::with_params("update_check_failed", vec![error.to_string()]).to_json()
        })?;
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|_| UiMessage::new("update_check_failed").to_json())?;
    let latest = json.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
    let latest_clean = latest.trim_start_matches('v');
    let is_newer = latest_clean
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect::<Vec<_>>()
        .cmp(
            &current
                .split('.')
                .map(|part| part.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>(),
        )
        == std::cmp::Ordering::Greater;
    Ok((is_newer && !latest.is_empty()).then(|| latest.to_string()))
}

/// 设置开机自启（写入 HKCU\Software\Microsoft\Windows\CurrentVersion\Run）。
/// 便携/自定义数据目录场景会附加 --data-dir 参数，保证自启用同一数据目录。
#[tauri::command]
pub fn set_auto_launch(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    let exe = std::env::current_exe().map_err(command_error)?;
    let mut command = format!("\"{}\"", exe.to_string_lossy());
    if paths.portable || !paths.root.to_string_lossy().is_empty() {
        // 数据目录若与默认 AppData 不同则显式传入。
        command.push_str(&format!(" --data-dir=\"{}\"", paths.root.to_string_lossy()));
    }
    if enabled {
        std::process::Command::new("reg")
            .args([
                "add",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                "/v",
                "MusicAutoSync",
                "/t",
                "REG_SZ",
                "/d",
                &command,
                "/f",
            ])
            .output()
            .map_err(command_error)?;
    } else {
        std::process::Command::new("reg")
            .args([
                "delete",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                "/v",
                "MusicAutoSync",
                "/f",
            ])
            .output()
            .map_err(command_error)?;
    }
    config.auto_launch = enabled;
    store::config::save(&paths.config_file, &config).map_err(command_error)
}

/// 清空同步日志/变更/删除/快照历史记录（不影响隔离区文件本身）。
#[tauri::command]
pub fn clear_sync_history_cmd(
    state: State<'_, AppState>,
    kind: String,
) -> Result<usize, String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    database::clear_history(&conn, &kind).map_err(command_error)
}

/// 计算恢复到历史快照的差异预览（不写网易）。
#[tauri::command]
pub async fn preview_playlist_restore_cmd(
    state: State<'_, AppState>,
    playlist_id: u64,
    history_id: i64,
) -> Result<sync::PlaylistRestoreDiff, String> {
    sync::preview_playlist_restore(&state, playlist_id, history_id)
        .await
        .map_err(|m| m.to_json())
}

/// 变更流水（每次同步的新增/删除记录）。
#[tauri::command]
pub fn get_sync_changes(
    state: State<'_, AppState>,
    limit: usize,
) -> Result<Vec<database::SyncChangeEntry>, String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    database::list_changes(&conn, limit.min(2000)).map_err(command_error)
}

/// 删除日志（本地被隔离 / 网易歌单被移除的曲目）。
#[tauri::command]
pub fn get_deleted_log(
    state: State<'_, AppState>,
    limit: usize,
) -> Result<Vec<database::DeletedLogEntry>, String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    database::list_deleted(&conn, limit.min(2000)).map_err(command_error)
}

/// 某歌单的历史快照列表。
#[tauri::command]
pub fn get_playlist_history(
    state: State<'_, AppState>,
    playlist_id: u64,
    limit: usize,
) -> Result<Vec<database::PlaylistHistoryEntry>, String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    database::list_playlist_history(&conn, playlist_id, limit.min(100)).map_err(command_error)
}

/// 恢复一条删除记录（本地隔离文件还原 / 网易曲目加回）。
#[tauri::command]
pub async fn restore_deleted_item(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<String, String> {
    if state.sync_running.load(Ordering::SeqCst) {
        return Err(UiMessage::new("sync_busy").to_json());
    }
    let _ = app.emit("sync://state", true);
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    let kind: Option<String> = conn
        .query_row("SELECT kind FROM deleted_log WHERE id=?1", [id], |r| r.get(0))
        .optional()
        .map_err(command_error)?;
    let kind = kind.ok_or_else(|| UiMessage::new("deleted_record_missing").to_json())?;
    let result = if kind == "local_file" {
        sync::restore_deleted_local_item(&state, id)
            .await
            .map_err(|m| m.to_json())
    } else {
        sync::restore_deleted_playlist_track(&state, id)
            .await
            .map_err(|m| m.to_json())
    };
    let _ = app.emit("sync://state", false);
    // 网易歌单曲目被加回 → 该歌单缓存失效。
    state.api_cache.invalidate_namespace("playlist_tracks");
    result
}

/// 把歌单恢复到某个历史快照。
#[tauri::command]
pub async fn restore_playlist_snapshot_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    playlist_id: u64,
    history_id: i64,
) -> Result<usize, String> {
    if state.sync_running.load(Ordering::SeqCst) {
        return Err(UiMessage::new("sync_busy").to_json());
    }
    let _ = app.emit("sync://state", true);
    let result = sync::restore_playlist_snapshot(&state, playlist_id, history_id)
        .await
        .map_err(|m| m.to_json());
    let _ = app.emit("sync://state", false);
    // 歌单内容被恢复改写 → 该歌单缓存失效。
    state.api_cache.invalidate_namespace("playlist_tracks");
    result
}

/// 账号统计（登录卡片展示）。字段取不到时置空，接口失败静默降级。
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountStats {
    pub nickname: Option<String>,
    pub user_id: Option<u64>,
    pub avatar_url: Option<String>,
    /// /user/level data 里的 level。
    pub level: Option<i64>,
    /// /vip/info(/v2) 的 redVipLevel 或 vipCode。
    pub vip_level: Option<i64>,
    /// /user/subcount 的 follows/followeds。
    pub follows: Option<i64>,
    pub followeds: Option<i64>,
    pub created_playlist_count: Option<i64>,
    pub subscribed_playlist_count: Option<i64>,
    /// 喜欢音乐数量（/likelist ids 长度）。
    pub liked_count: Option<u64>,
    pub event_count: Option<i64>,
}

#[tauri::command]
pub async fn get_account_stats(
    state: State<'_, AppState>,
    force: Option<bool>,
) -> Result<AccountStats, String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    let user = config
        .cookie_user
        .as_ref()
        .context("请先登录")
        .map_err(command_error)?;
    // 账号资料低频变化：共享缓存 + TTL（user_detail/user_level/subcount/vip/likelist）。
    // force=true（登录页“刷新统计”按钮）穿透全部缓存直拉并回填。
    let api = cached_api(&state, &config)?;
    let force = force.unwrap_or(false);
    let mut stats = AccountStats {
        nickname: Some(user.nickname.clone()),
        user_id: Some(user.user_id),
        ..AccountStats::default()
    };

    // 头像/昵称/关注/粉丝/动态来自 user/detail 的 profile。
    let detail = if force {
        api.user_detail_forced(user.user_id).await
    } else {
        api.user_detail(user.user_id).await
    };
    if let Ok(detail) = detail {
        if !detail.is_null() {
            stats.avatar_url = detail.get("avatarUrl").and_then(|v| v.as_str()).map(str::to_owned);
            stats.nickname = detail.get("nickname").and_then(|v| v.as_str()).map(str::to_owned);
            stats.follows = detail.get("follows").and_then(value_as_i64);
            stats.followeds = detail.get("followeds").and_then(value_as_i64);
            stats.event_count = detail.get("eventCount").and_then(value_as_i64);
        }
    }
    // 等级来自 /user/level 的 data.level。
    let level = if force {
        api.user_level_forced().await
    } else {
        api.user_level().await
    };
    if let Ok(level) = level {
        if !level.is_null() {
            stats.level = level.get("level").and_then(value_as_i64);
        }
    }
    // 歌单创建/收藏数量来自 /user/subcount（字段在顶层，非 data）。
    let subcount = if force {
        api.user_subcount_forced().await
    } else {
        api.user_subcount().await
    };
    if let Ok(subcount) = subcount {
        if !subcount.is_null() {
            stats.created_playlist_count = subcount
                .get("createdPlaylistCount")
                .and_then(value_as_i64);
            stats.subscribed_playlist_count = subcount
                .get("subPlaylistCount")
                .and_then(value_as_i64);
        }
    }
    let vip = if force {
        api.vip_info_forced().await
    } else {
        api.vip_info().await
    };
    if let Ok(vip) = vip {
        if !vip.is_null() {
            stats.vip_level = vip
                .get("redVipLevel")
                .or_else(|| vip.get("vipCode"))
                .and_then(value_as_i64);
        }
    }
    let liked = if force {
        api.liked_song_ids_forced(user.user_id).await
    } else {
        api.liked_song_ids(user.user_id).await
    };
    if let Ok(ids) = liked {
        stats.liked_count = Some(ids.len() as u64);
    }
    Ok(stats)
}

fn value_as_i64(v: &serde_json::Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_u64().map(|x| x as i64))
}

/// 本地同步统计（登录卡片 / 概览）。
#[tauri::command]
pub fn get_local_stats(state: State<'_, AppState>) -> Result<database::LocalStats, String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    let mut stats = database::summarize_stats(&conn).map_err(command_error)?;
    // 精确统计“磁盘上仍存在的已登记文件”。
    let mut stmt = conn
        .prepare("SELECT local_path FROM track_files")
        .map_err(command_error)?;
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .map_err(command_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(command_error)?;
    stats.current_local_files = rows
        .iter()
        .filter(|p| Path::new(p).is_file())
        .count() as u64;
    Ok(stats)
}

/// NCM 批量转换汇总结果。
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NcmConvertReport {
    pub converted: usize,
    pub skipped: usize,
    pub failed: usize,
    pub items: Vec<crate::ncm::ncm::NcmConvertItemResult>,
}

/// 独立 NCM 转换工具：paths 可混合 .ncm 文件与目录（目录递归）。
/// keep_source=false 时转换成功后删除源文件；overwrite=true 时无视已有转换标记。
#[tauri::command]
pub async fn convert_ncm_manual(
    state: State<'_, AppState>,
    paths: Vec<String>,
    keep_source: bool,
    overwrite: bool,
) -> Result<NcmConvertReport, String> {
    let _ = state;
    // 展开为 .ncm 文件列表。
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for p in &paths {
        let path = std::path::Path::new(p);
        if path.is_dir() {
            for entry in walkdir::WalkDir::new(path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let entry_path = entry.path();
                if entry_path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("ncm"))
                {
                    files.push(entry_path.to_path_buf());
                }
            }
        } else if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ncm"))
        {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err(UiMessage::new("ncm_no_files").to_json());
    }
    let report = tauri::async_runtime::spawn_blocking(move || {
        let mut report = NcmConvertReport::default();
        for file in files {
            let item = crate::ncm::ncm::convert_file_with_marker(&file, keep_source, overwrite);
            match item.status.as_str() {
                "converted" => report.converted += 1,
                "skipped" => report.skipped += 1,
                _ => report.failed += 1,
            }
            report.items.push(item);
        }
        report
    })
    .await
    .map_err(|error| UiMessage::with_params("ncm_convert_failed", vec![error.to_string()]).to_json())?;
    Ok(report)
}
