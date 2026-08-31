use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{fs, path::Path, sync::atomic::Ordering};
use tauri::{AppHandle, State};

use crate::{
    api::{LoginStatus, NeteaseApi, PlaylistInfo, QrCheckResult, QrSession},
    core::sync::{self, SyncReport},
    store::{self, config::{Config, CookieUser, PlaylistSyncSetting}, database},
    AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    data_dir: String,
    data_dir_portable: bool,
    version: String,
}

fn command_error(error: impl std::fmt::Display) -> String { error.to_string() }

#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> AppInfo {
    let paths = state.paths.get();
    AppInfo { data_dir: paths.root.to_string_lossy().into(), data_dir_portable: paths.portable, version: env!("CARGO_PKG_VERSION").into() }
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
pub fn set_data_dir(state: State<'_, AppState>, dir: String, migrate: bool) -> Result<AppInfo, String> {
    let old = state.paths.get();
    let new = store::paths::DataPaths::from_root(dir.into(), true, old.exe_dir.clone()).map_err(command_error)?;
    if migrate && old.root != new.root {
        copy_dir_contents(&old.root, &new.root).map_err(command_error)?;
    }
    new.write_portable_marker(&new.root).map_err(command_error)?;
    state.paths.set(new.clone());
    Ok(AppInfo { data_dir: new.root.to_string_lossy().into(), data_dir_portable: true, version: env!("CARGO_PKG_VERSION").into() })
}

#[tauri::command]
pub async fn get_login_qr(state: State<'_, AppState>) -> Result<QrSession, String> {
    let config = store::config::load(&state.paths.get().config_file).map_err(command_error)?;
    NeteaseApi::from_config(&config).map_err(command_error)?.login_qr().await.map_err(command_error)
}

#[tauri::command]
pub async fn check_login_qr(state: State<'_, AppState>, key: String) -> Result<QrCheckResult, String> {
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file).map_err(command_error)?;
    let api = NeteaseApi::from_config(&config).map_err(command_error)?;
    let (result, cookie) = api.check_qr(&key).await.map_err(command_error)?;
    if result.state == "success" {
        let cookie = cookie.context("登录成功但服务未返回 Cookie").map_err(command_error)?;
        let mut config = config;
        config.cookie = Some(cookie);
        let profile = NeteaseApi::from_config(&config).map_err(command_error)?.login_status().await.map_err(command_error)?;
        config.cookie_user = match (profile.user_id, profile.nickname) {
            (Some(user_id), Some(nickname)) => Some(CookieUser { user_id, nickname }),
            _ => None,
        };
        store::config::save(&paths.config_file, &config).map_err(command_error)?;
    }
    Ok(result)
}

#[tauri::command]
pub async fn get_login_status(state: State<'_, AppState>) -> Result<LoginStatus, String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    let status = NeteaseApi::from_config(&config).map_err(command_error)?.login_status().await.map_err(command_error)?;
    if status.logged_in {
        if let (Some(id), Some(nickname)) = (status.user_id, status.nickname.clone()) {
            if config.cookie_user.as_ref().is_none_or(|u| u.user_id != id || u.nickname != nickname) {
                config.cookie_user = Some(CookieUser { user_id: id, nickname });
                store::config::save(&paths.config_file, &config).map_err(command_error)?;
            }
        }
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
    let user = config.cookie_user.as_ref().context("请先登录").map_err(command_error)?;
    NeteaseApi::from_config(&config).map_err(command_error)?.user_playlists(user.user_id, &config).await.map_err(command_error)
}

#[tauri::command]
pub fn set_playlist_enabled(state: State<'_, AppState>, id: u64, enabled: bool) -> Result<(), String> {
    let paths = state.paths.get();
    let mut config = store::config::load(&paths.config_file).map_err(command_error)?;
    if let Some(setting) = config.playlists.iter_mut().find(|p| p.id == id) {
        setting.enabled = enabled;
    } else {
        config.playlists.push(PlaylistSyncSetting { id, name: format!("歌单 {id}"), enabled, folder_override: None, quality_override: None });
    }
    store::config::save(&paths.config_file, &config).map_err(command_error)
}

#[tauri::command]
pub async fn sync_playlist(app: AppHandle, state: State<'_, AppState>, id: u64) -> Result<SyncReport, String> {
    sync::sync_one(&app, &state, id).await.map_err(command_error)
}

#[tauri::command]
pub async fn sync_all(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<SyncReport>, String> {
    sync::sync_enabled(&app, &state).await.map_err(command_error)
}

#[tauri::command]
pub fn cancel_sync(state: State<'_, AppState>) -> bool {
    if state.sync_running.load(Ordering::SeqCst) {
        state.cancel_requested.store(true, Ordering::SeqCst);
        true
    } else { false }
}

#[tauri::command]
pub fn get_sync_logs(state: State<'_, AppState>, limit: usize) -> Result<Vec<database::SyncLogEntry>, String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    database::get_logs(&conn, limit.min(500)).map_err(command_error)
}

#[tauri::command]
pub fn list_quarantine(state: State<'_, AppState>) -> Result<Vec<database::QuarantineItem>, String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    database::list_quarantine(&conn).map_err(command_error)
}

#[tauri::command]
pub fn restore_quarantine(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    let item: (String, String) = conn.query_row("SELECT original_path, quarantine_path FROM quarantine WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(command_error)?;
    let original = Path::new(&item.0);
    let quarantined = Path::new(&item.1);
    if !quarantined.is_file() { return Err("隔离文件已不存在".into()); }
    if original.exists() { return Err("原路径已有同名文件，不能覆盖".into()); }
    if let Some(parent) = original.parent() { fs::create_dir_all(parent).map_err(command_error)?; }
    fs::rename(quarantined, original).map_err(command_error)?;
    conn.execute("DELETE FROM quarantine WHERE id=?1", [id]).map_err(command_error)?;
    Ok(())
}

#[tauri::command]
pub fn delete_quarantine(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = database::open(&state.paths.get().database_file).map_err(command_error)?;
    let path: Option<String> = conn.query_row("SELECT quarantine_path FROM quarantine WHERE id=?1", [id], |r| r.get(0)).optional().map_err(command_error)?;
    let path = path.context("隔离记录不存在").map_err(command_error)?;
    if Path::new(&path).is_file() { fs::remove_file(&path).map_err(command_error)?; }
    conn.execute("DELETE FROM quarantine WHERE id=?1", [id]).map_err(command_error)?;
    Ok(())
}

fn copy_dir_contents(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() { copy_dir_contents(&source, &target)?; }
        else if !target.exists() { fs::copy(&source, &target)?; }
    }
    Ok(())
}
