use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{collections::HashMap, fs, path::Path, sync::atomic::Ordering};
use tauri::{AppHandle, Manager, State};

use crate::{
    api::{
        ApiResponseMeta, LoginStatus, LoginStatusResponse, NeteaseApi, PlaylistInfo, QrCheckResult,
        QrSession,
    },
    core::{
        naming,
        sync::{self, SyncReport},
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
    store::config::save(&paths.config_file, &config).map_err(command_error)
}

#[tauri::command]
pub async fn list_playlists(state: State<'_, AppState>) -> Result<Vec<PlaylistInfo>, String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    let user = config
        .cookie_user
        .as_ref()
        .context("请先登录")
        .map_err(command_error)?;
    let mut playlists = NeteaseApi::from_config(&config)
        .map_err(command_error)?
        .user_playlists(user.user_id, &config)
        .await
        .map_err(command_error)?;
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
) -> Result<PlaylistSongsResult, String> {
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file).map_err(command_error)?;
    let playlist = NeteaseApi::from_config(&config)
        .map_err(command_error)?
        .playlist_tracks(id)
        .await
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
        songs.push(PlaylistSong {
            id: track.id,
            name: track.name.clone(),
            artists: naming::artists_with(track, &config.artist_separator),
            album: track.al.name.clone(),
            duration_ms: track.dt,
            position: index + 1,
            synced: local_path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_file()),
            local_path,
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
    sync::sync_one(&app, &state, id)
        .await
        .map_err(|message| message.to_json())
}

#[tauri::command]
pub async fn sync_all(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SyncReport>, String> {
    sync::sync_enabled(&app, &state)
        .await
        .map_err(|message| message.to_json())
}

#[tauri::command]
pub fn cancel_sync(state: State<'_, AppState>) -> bool {
    if state.sync_running.load(Ordering::SeqCst) {
        state.cancel_requested.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
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
