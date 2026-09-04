use anyhow::{anyhow, Context, Result};
use chrono::Local;
use reqwest::Client;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

use crate::{
    api::{NeteaseApi, PlaylistTracks, Track},
    core::naming,
    error::UiMessage,
    store::{self, config::Config, database},
    tags::tags,
    AppState,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub playlist_id: Option<u64>,
    pub playlist_name: String,
    pub phase: String,
    pub current: usize,
    pub total: usize,
    pub message: UiMessage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub playlist_id: u64,
    pub playlist_name: String,
    pub added: usize,
    pub updated: usize,
    pub quarantined: usize,
    pub ncm_converted: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: Vec<UiMessage>,
    pub started_at: String,
    pub finished_at: String,
}

pub async fn sync_one(
    app: &AppHandle,
    state: &AppState,
    playlist_id: u64,
) -> Result<SyncReport, UiMessage> {
    if state.sync_running.swap(true, Ordering::SeqCst) {
        return Err(UiMessage::new("sync_busy"));
    }
    let _ = app.emit("sync://state", true);
    let result = sync_one_inner(Some(app), state, playlist_id).await;
    state.sync_running.store(false, Ordering::SeqCst);
    let _ = app.emit("sync://state", false);
    match result {
        Ok(report) => {
            let _ = app.emit("sync://report", &report);
            Ok(report)
        }
        Err(error) => Err(ui_from_error(error)),
    }
}

pub async fn sync_enabled(app: &AppHandle, state: &AppState) -> Result<Vec<SyncReport>, UiMessage> {
    if state.sync_running.swap(true, Ordering::SeqCst) {
        return Err(UiMessage::new("sync_busy"));
    }
    let _ = app.emit("sync://state", true);
    let config = store::config::load(&state.paths.get().config_file).map_err(UiMessage::unknown)?;
    let ids: Vec<u64> = config
        .playlists
        .iter()
        .filter(|p| p.enabled)
        .map(|p| p.id)
        .collect();
    let mut reports = Vec::new();
    for id in ids {
        if state.cancel_requested.load(Ordering::SeqCst) {
            break;
        }
        match sync_one_inner(Some(app), state, id).await {
            Ok(report) => {
                let _ = app.emit("sync://report", &report);
                reports.push(report);
            }
            Err(error) => tracing::error!(%error, "playlist sync failed"),
        }
    }
    state.sync_running.store(false, Ordering::SeqCst);
    let _ = app.emit("sync://state", false);
    Ok(reports)
}

fn ui_from_error(error: anyhow::Error) -> UiMessage {
    error
        .downcast_ref::<UiMessage>()
        .cloned()
        .unwrap_or_else(|| UiMessage::unknown(error))
}

/// CLI 无窗口模式：直接执行单个歌单同步，不发送事件。
pub async fn cli_sync(state: &AppState, playlist_id: u64) -> Result<SyncReport, UiMessage> {
    sync_one_inner(None, state, playlist_id)
        .await
        .map_err(ui_from_error)
}

async fn sync_one_inner(
    app: Option<&AppHandle>,
    state: &AppState,
    playlist_id: u64,
) -> Result<SyncReport> {
    state.cancel_requested.store(false, Ordering::SeqCst);
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file)?;
    let root = config
        .music_root
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!(UiMessage::new("music_root_required")))?;
    let api = NeteaseApi::from_config(&config)?;
    emit_progress(
        app,
        playlist_id,
        "",
        "phase_read_playlist",
        0,
        0,
        UiMessage::new("read_playlist"),
    );
    let playlist = api.playlist_tracks(playlist_id).await?;
    let setting = config.playlists.iter().find(|p| p.id == playlist_id);
    let quality = setting
        .and_then(|x| x.quality_override.as_deref())
        .unwrap_or(&config.quality);
    let folder_template = setting
        .and_then(|x| x.folder_override.as_deref())
        .unwrap_or(&config.folder_template);
    let artist_separator = &config.artist_separator;
    let now = database::now();
    let mut report = SyncReport {
        playlist_id,
        playlist_name: playlist.name.clone(),
        added: 0,
        updated: 0,
        quarantined: 0,
        ncm_converted: 0,
        failed: 0,
        skipped: 0,
        errors: vec![],
        started_at: now,
        finished_at: String::new(),
    };
    let mut conn = database::open(&paths.database_file)?;
    database::log(
        &conn,
        &playlist.name,
        "running",
        &UiMessage::new("sync_start").to_json(),
    )?;

    convert_ncm_files(app, state, &config, &playlist, &mut report).await?;
    sync_tracks(
        app,
        state,
        &api,
        &config,
        &playlist,
        &root,
        folder_template,
        quality,
        artist_separator,
        &mut conn,
        &paths.logs_dir,
        &mut report,
    )
    .await?;
    quarantine_removed(
        app,
        &playlist,
        &root,
        folder_template,
        artist_separator,
        &mut conn,
        &mut report,
    )?;
    if config.write_m3u8 {
        write_m3u8(&playlist, &root, folder_template, artist_separator, &conn)?;
    }

    report.finished_at = database::now();
    let status = if report.failed == 0 { "ok" } else { "error" };
    database::log(
        &conn,
        &playlist.name,
        status,
        &UiMessage::with_params(
            "sync_done",
            vec![
                report.added.to_string(),
                report.quarantined.to_string(),
                report.failed.to_string(),
            ],
        )
        .to_json(),
    )?;
    database::record_sync_run(&conn, &report)?;
    Ok(report)
}

async fn sync_tracks(
    app: Option<&AppHandle>,
    state: &AppState,
    api: &NeteaseApi,
    config: &Config,
    playlist: &PlaylistTracks,
    root: &Path,
    folder_template: &str,
    quality: &str,
    artist_separator: &str,
    conn: &mut rusqlite::Connection,
    logs_dir: &Path,
    report: &mut SyncReport,
) -> Result<()> {
    let mut expected = HashSet::new();
    let overwrite = config
        .playlists
        .iter()
        .find(|setting| setting.id == playlist.id)
        .map(|setting| setting.overwrite)
        .unwrap_or(false);
    for (index, track) in playlist.tracks.iter().enumerate() {
        if state.cancel_requested.load(Ordering::SeqCst) {
            return Err(anyhow!(UiMessage::new("sync_canceled")));
        }
        expected.insert(track.id);
        emit_progress(
            app,
            playlist.id,
            &playlist.name,
            "phase_download",
            index + 1,
            playlist.tracks.len(),
            UiMessage::with_params("track", vec![track.name.clone()]),
        );
        match sync_one_track(
            api,
            config,
            playlist,
            root,
            folder_template,
            quality,
            artist_separator,
            conn,
            logs_dir,
            track,
            index + 1,
            overwrite,
        )
        .await
        {
            TrackOutcome::Skipped => report.skipped += 1,
            TrackOutcome::Downloaded => report.added += 1,
            TrackOutcome::Failed(message) => {
                report.failed += 1;
                report.errors.push(message);
            }
        }
    }
    Ok(())
}

enum TrackOutcome {
    Skipped,
    Downloaded,
    Failed(UiMessage),
}

fn record_track_file(
    conn: &mut rusqlite::Connection,
    playlist: &PlaylistTracks,
    track: &Track,
    target: &Path,
    extension: &str,
) -> Result<()> {
    let timestamp = database::now();
    conn.execute(
        "INSERT INTO track_files(playlist_id,track_id,local_path,source_format,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)
         ON CONFLICT(playlist_id,track_id) DO UPDATE SET local_path=excluded.local_path,source_format=excluded.source_format,updated_at=excluded.updated_at",
        params![playlist.id, track.id, target.to_string_lossy(), extension, timestamp],
    )?;
    Ok(())
}

async fn finalize_track(
    api: &NeteaseApi,
    playlist: &PlaylistTracks,
    track: &Track,
    position: usize,
    target: &Path,
    extension: &str,
    write_lrc: bool,
    artist_separator: &str,
    conn: &mut rusqlite::Connection,
) {
    if let Err(error) = tags::write_basic_tags(target, track, position, track.id, artist_separator)
    {
        tracing::warn!(%error, path = %target.display(), "metadata write failed");
    }
    if write_lrc {
        if let Ok(Some(lyrics)) = api.lyric(track.id).await {
            let _ = fs::write(target.with_extension("lrc"), lyrics);
        }
    }
    let _ = record_track_file(conn, playlist, track, target, extension);
    let _ = write_sidecar(target, playlist.id, track.id);
    let _ = update_snapshot(conn, playlist.id, track.id, position - 1);
}

async fn sync_one_track(
    api: &NeteaseApi,
    config: &Config,
    playlist: &PlaylistTracks,
    root: &Path,
    folder_template: &str,
    quality: &str,
    artist_separator: &str,
    conn: &mut rusqlite::Connection,
    logs_dir: &Path,
    track: &Track,
    position: usize,
    overwrite: bool,
) -> TrackOutcome {
    let mut log_entry = store::track_log::TrackLogEntry::new(
        Some(playlist.id),
        &playlist.name,
        track.id,
        &track.name,
    );
    if !overwrite {
        let known_path: Option<String> = conn
            .query_row(
                "SELECT local_path FROM track_files WHERE playlist_id=?1 AND track_id=?2",
                params![playlist.id, track.id],
                |row| row.get(0),
            )
            .optional()
            .unwrap_or(None);
        if known_path
            .as_deref()
            .is_some_and(|path| Path::new(path).is_file())
        {
            let _ = update_snapshot(conn, playlist.id, track.id, position - 1);
            log_entry.outcome("skipped", &UiMessage::new("track_exists"));
            let _ = store::track_log::append(logs_dir, &log_entry);
            return TrackOutcome::Skipped;
        }
    }

    let (download_url, extension) = match fetch_download_url(api, track.id, quality).await {
        Ok(Some(download)) => download,
        Ok(None) => {
            let message = UiMessage::with_params("no_url", vec![track.name.clone()]);
            log_entry.outcome("failed", &message);
            let _ = store::track_log::append(logs_dir, &log_entry);
            return TrackOutcome::Failed(message);
        }
        Err(error) => {
            let message = UiMessage::with_params("song_url_failed", vec![error.to_string()]);
            log_entry.outcome("failed", &message);
            let _ = store::track_log::append(logs_dir, &log_entry);
            return TrackOutcome::Failed(message);
        }
    };
    let target = naming::track_path(
        root,
        folder_template,
        &config.filename_template,
        &playlist.name,
        track,
        position,
        &extension,
        artist_separator,
    );

    if !overwrite && target.is_file() {
        // 本地已存在按当前模板命名的文件：登记为已同步并跳过下载，避免覆盖用户文件。
        let _ = record_track_file(conn, playlist, track, &target, &extension);
        let _ = write_sidecar(&target, playlist.id, track.id);
        let _ = update_snapshot(conn, playlist.id, track.id, position - 1);
        log_entry.outcome("skipped", &UiMessage::new("track_exists"));
        let _ = store::track_log::append(logs_dir, &log_entry);
        return TrackOutcome::Skipped;
    }

    match download_track(&download_url, &target).await {
        Ok(bytes) => {
            finalize_track(
                api,
                playlist,
                track,
                position,
                &target,
                &extension,
                config.write_lrc,
                artist_separator,
                conn,
            )
            .await;
            log_entry.done(&target, bytes, quality);
            let _ = store::track_log::append(logs_dir, &log_entry);
            TrackOutcome::Downloaded
        }
        Err(error) => {
            let message = UiMessage::with_params("download_failed", vec![error.to_string()]);
            log_entry.outcome("failed", &message);
            let _ = store::track_log::append(logs_dir, &log_entry);
            TrackOutcome::Failed(message)
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleDownloadOptions {
    pub target_dir: Option<String>,
    pub filename_template: Option<String>,
    pub quality: Option<String>,
    pub write_lrc: Option<bool>,
    pub overwrite: bool,
}

pub async fn download_song_with_options(
    app: Option<&AppHandle>,
    state: &AppState,
    playlist_id: u64,
    track_id: u64,
    options: SingleDownloadOptions,
) -> Result<String, UiMessage> {
    if state.sync_running.load(Ordering::SeqCst) {
        return Err(UiMessage::new("sync_busy"));
    }
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file).map_err(UiMessage::unknown)?;
    let api = NeteaseApi::from_config(&config).map_err(UiMessage::unknown)?;
    let playlist = api.playlist_tracks(playlist_id).await.map_err(|error| {
        UiMessage::with_params("playlist_fetch_failed", vec![error.to_string()])
    })?;
    let (index, track) = playlist
        .tracks
        .iter()
        .enumerate()
        .find(|(_, track)| track.id == track_id)
        .ok_or_else(|| UiMessage::new("track_missing"))?;
    let setting = config
        .playlists
        .iter()
        .find(|playlist| playlist.id == playlist_id);
    let quality = options
        .quality
        .as_deref()
        .or_else(|| setting.and_then(|setting| setting.quality_override.as_deref()))
        .unwrap_or(&config.quality);
    let folder_template = setting
        .and_then(|setting| setting.folder_override.as_deref())
        .unwrap_or(&config.folder_template);
    let filename_template = options
        .filename_template
        .as_deref()
        .unwrap_or(&config.filename_template);
    let write_lrc = options.write_lrc.unwrap_or(config.write_lrc);
    let artist_separator = &config.artist_separator;
    let mut conn = database::open(&paths.database_file).map_err(UiMessage::unknown)?;

    if let Some(app) = app {
        let _ = app.emit(
            "sync://progress",
            SyncProgress {
                playlist_id: Some(playlist_id),
                playlist_name: playlist.name.clone(),
                phase: "phase_download".into(),
                current: index + 1,
                total: playlist.tracks.len(),
                message: UiMessage::with_params("track", vec![track.name.clone()]),
            },
        );
    }

    let (download_url, extension) = match fetch_download_url(&api, track.id, quality).await {
        Ok(Some(download)) => download,
        Ok(None) => return Err(UiMessage::with_params("no_url", vec![track.name.clone()])),
        Err(error) => {
            return Err(UiMessage::with_params(
                "song_url_failed",
                vec![error.to_string()],
            ))
        }
    };
    let file_name = naming::apply_template(
        filename_template,
        &playlist.name,
        track,
        index + 1,
        artist_separator,
    );
    let mut in_music_root = true;
    let target = match options.target_dir.as_deref().filter(|dir| !dir.is_empty()) {
        Some(dir) => {
            in_music_root = false;
            PathBuf::from(dir).join(format!("{file_name}.{extension}"))
        }
        None => {
            // 未指定保存目录时才要求音乐根目录。
            let root = PathBuf::from(
                config
                    .music_root
                    .as_deref()
                    .ok_or_else(|| UiMessage::new("music_root_required"))?,
            );
            naming::track_path(
                &root,
                folder_template,
                filename_template,
                &playlist.name,
                track,
                index + 1,
                &extension,
                artist_separator,
            )
        }
    };
    if target.exists() && !options.overwrite {
        return Err(UiMessage::with_params(
            "file_exists",
            vec![target.to_string_lossy().into_owned()],
        ));
    }
    let mut log_entry = store::track_log::TrackLogEntry::new(
        Some(playlist_id),
        &playlist.name,
        track.id,
        &track.name,
    );
    match download_track(&download_url, &target).await {
        Ok(bytes) => {
            log_entry.done(&target, bytes, quality);
            let _ = store::track_log::append(&paths.logs_dir, &log_entry);
        }
        Err(error) => {
            let message = UiMessage::with_params("download_failed", vec![error.to_string()]);
            log_entry.outcome("failed", &message);
            let _ = store::track_log::append(&paths.logs_dir, &log_entry);
            return Err(message);
        }
    }
    if in_music_root {
        // 只有下载到音乐根目录下才登记为“已同步”。
        finalize_track(
            &api,
            &playlist,
            track,
            index + 1,
            &target,
            &extension,
            write_lrc,
            artist_separator,
            &mut conn,
        )
        .await;
    } else if write_lrc {
        if let Ok(Some(lyrics)) = api.lyric(track.id).await {
            let _ = fs::write(target.with_extension("lrc"), lyrics);
        }
    }
    if let Err(error) =
        tags::write_basic_tags(&target, track, index + 1, track.id, artist_separator)
    {
        tracing::warn!(%error, path = %target.display(), "metadata write failed");
    }
    Ok(target.to_string_lossy().into_owned())
}

async fn download_track(url: &str, target: &Path) -> Result<u64> {
    let parent = target.parent().context("target has no parent")?;
    fs::create_dir_all(parent)?;
    let part = target.with_extension(format!(
        "{}.part",
        target.extension().and_then(|x| x.to_str()).unwrap_or("mp3")
    ));
    let client = Client::builder().user_agent("Mozilla/5.0").build()?;
    let response = client.get(url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;
    if bytes.len() < 1024 {
        return Err(anyhow!(UiMessage::new("download_small_file")));
    }
    fs::write(&part, &bytes)?;
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(part, target)?;
    Ok(bytes.len() as u64)
}

fn update_snapshot(
    conn: &rusqlite::Connection,
    playlist_id: u64,
    track_id: u64,
    position: usize,
) -> Result<()> {
    conn.execute("INSERT INTO playlist_snapshots(playlist_id,track_id,position,snapshot_at) VALUES(?1,?2,?3,?4)
        ON CONFLICT(playlist_id,track_id) DO UPDATE SET position=excluded.position,snapshot_at=excluded.snapshot_at",
        params![playlist_id, track_id, position, database::now()])?;
    Ok(())
}

async fn fetch_download_url(
    api: &NeteaseApi,
    track_id: u64,
    quality: &str,
) -> Result<Option<(String, String)>> {
    let mut picked: Option<(String, String)> = None;
    // 依据设置决定优先使用哪个接口家族。
    let prefer_download_api = matches!(api.download_source(), "download" | "download-first");
    if prefer_download_api {
        picked = api.song_download_url_candidate(track_id).await;
    }
    if picked.is_none() {
        for level in quality_fallback_chain(quality) {
            if let Ok(song_url) = api.song_url(track_id, level).await {
                if let Some(url) = song_url.url {
                    picked = Some((url, song_url.file_type.unwrap_or_else(|| "mp3".into())));
                    break;
                }
            }
        }
    }
    if picked.is_none() && !prefer_download_api {
        // 兜底：个别实例 /song/url/v1 拿不到地址时，尝试下载接口候选。
        picked = api.song_download_url_candidate(track_id).await;
    }
    // 网易 CDN 支持 https，明文 http 直链在部分网络/代理下会被拒绝，统一升级。
    Ok(picked.map(|(url, format)| (url.replace("http://", "https://"), format)))
}

fn quality_fallback_chain(quality: &str) -> &'static [&'static str] {
    match quality {
        "hires" => &["hires", "lossless", "exhigh", "higher", "standard"],
        "lossless" => &["lossless", "exhigh", "higher", "standard"],
        "exhigh" => &["exhigh", "higher", "standard"],
        "higher" => &["higher", "standard"],
        _ => &["standard"],
    }
}

#[allow(clippy::too_many_arguments)]
fn quarantine_removed(
    app: Option<&AppHandle>,
    playlist: &PlaylistTracks,
    root: &Path,
    folder_template: &str,
    artist_separator: &str,
    conn: &mut rusqlite::Connection,
    report: &mut SyncReport,
) -> Result<()> {
    let ids: HashSet<u64> = playlist.tracks.iter().map(|t| t.id).collect();
    let mut stmt =
        conn.prepare("SELECT track_id,local_path FROM track_files WHERE playlist_id=?1")?;
    let stale: Vec<(u64, String)> = stmt
        .query_map([playlist.id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    drop(stmt);
    let playlist_folder = naming::sanitize_component(&naming::apply_template(
        folder_template,
        &playlist.name,
        &playlist.tracks.first().cloned().unwrap_or_else(empty_track),
        1,
        artist_separator,
    ));
    for (track_id, local_path) in stale {
        if ids.contains(&track_id) {
            continue;
        }
        let source = PathBuf::from(&local_path);
        if source.is_file() {
            emit_progress(
                app,
                playlist.id,
                &playlist.name,
                "phase_quarantine",
                report.quarantined,
                0,
                UiMessage::with_params(
                    "track",
                    vec![source
                        .file_name()
                        .and_then(|v| v.to_str())
                        .unwrap_or_default()
                        .to_owned()],
                ),
            );
            let quarantine_dir = root.join(".quarantine").join(&playlist_folder);
            fs::create_dir_all(&quarantine_dir)?;
            let target = unique_quarantine_path(&quarantine_dir, &source);
            fs::rename(&source, &target)?;
            let lrc = source.with_extension("lrc");
            if lrc.is_file() {
                let _ = fs::rename(lrc, target.with_extension("lrc"));
            }
            let sidecar = sidecar_path(&source);
            if sidecar.is_file() {
                let _ = fs::rename(sidecar, sidecar_path(&target));
            }
            database::add_quarantine(
                conn,
                playlist.id,
                &playlist.name,
                source
                    .file_name()
                    .and_then(|x| x.to_str())
                    .unwrap_or_default(),
                &local_path,
                &target.to_string_lossy(),
            )?;
            report.quarantined += 1;
        }
        conn.execute(
            "DELETE FROM track_files WHERE playlist_id=?1 AND track_id=?2",
            params![playlist.id, track_id],
        )?;
        conn.execute(
            "DELETE FROM playlist_snapshots WHERE playlist_id=?1 AND track_id=?2",
            params![playlist.id, track_id],
        )?;
    }
    Ok(())
}

fn write_m3u8(
    playlist: &PlaylistTracks,
    root: &Path,
    folder_template: &str,
    artist_separator: &str,
    conn: &rusqlite::Connection,
) -> Result<()> {
    let folder = root.join(naming::apply_template(
        folder_template,
        &playlist.name,
        &playlist.tracks.first().cloned().unwrap_or_else(empty_track),
        1,
        artist_separator,
    ));
    fs::create_dir_all(&folder)?;
    let mut out = String::from("#EXTM3U\n");
    for track in &playlist.tracks {
        let path: Option<String> = conn
            .query_row(
                "SELECT local_path FROM track_files WHERE playlist_id=?1 AND track_id=?2",
                params![playlist.id, track.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(path) = path {
            out.push_str(&format!("{}\n", path));
        }
    }
    fs::write(folder.join("playlist.m3u8"), out)?;
    Ok(())
}

async fn convert_ncm_files(
    app: Option<&AppHandle>,
    _state: &AppState,
    config: &Config,
    playlist: &PlaylistTracks,
    report: &mut SyncReport,
) -> Result<()> {
    if !config.ncm_convert {
        return Ok(());
    }
    let mut scan_dirs = config.ncm_scan_dirs.clone();
    if let Some(root) = &config.music_root {
        scan_dirs.push(root.clone());
    }
    scan_dirs.sort();
    scan_dirs.dedup();

    for root in scan_dirs {
        let root = PathBuf::from(root);
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path
                .extension()
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("ncm"))
            {
                continue;
            }
            let converted_marker = path.with_extension("ncm.converted.json");
            if converted_marker.exists() {
                continue;
            }
            emit_progress(
                app,
                playlist.id,
                &playlist.name,
                "phase_convert_ncm",
                report.ncm_converted,
                0,
                UiMessage::with_params(
                    "track",
                    vec![path
                        .file_name()
                        .and_then(|x| x.to_str())
                        .unwrap_or_default()
                        .to_owned()],
                ),
            );
            let output_dir = path.parent().context("NCM 文件缺少上级目录")?;
            match crate::ncm::ncm::convert(path, output_dir) {
                Ok(output) => {
                    let marker = serde_json::json!({
                        "source": path.to_string_lossy(),
                        "output": output.path.to_string_lossy(),
                        "convertedAt": database::now(),
                        "format": output.metadata.format,
                    });
                    fs::write(converted_marker, serde_json::to_vec_pretty(&marker)?)?;
                    report.ncm_converted += 1;
                    if !config.ncm_keep_source && path.is_file() {
                        let _ = fs::remove_file(path);
                    }
                }
                Err(error) => {
                    report.errors.push(UiMessage::with_params(
                        "ncm_convert_failed",
                        vec![path.to_string_lossy().into_owned(), error.to_string()],
                    ));
                    tracing::warn!(%error, path = %path.display(), "NCM conversion failed");
                }
            }
        }
    }
    Ok(())
}

fn emit_progress(
    app: Option<&AppHandle>,
    playlist_id: u64,
    playlist_name: &str,
    phase: &str,
    current: usize,
    total: usize,
    message: UiMessage,
) {
    if let Some(app) = app {
        let _ = app.emit(
            "sync://progress",
            SyncProgress {
                playlist_id: Some(playlist_id),
                playlist_name: playlist_name.to_owned(),
                phase: phase.to_owned(),
                current,
                total,
                message,
            },
        );
    }
}

fn write_sidecar(path: &Path, playlist_id: u64, track_id: u64) -> Result<()> {
    fs::write(
        sidecar_path(path),
        serde_json::json!({ "neteaseId": track_id, "playlistId": playlist_id }).to_string(),
    )?;
    Ok(())
}

fn sidecar_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}.netease.json",
        path.extension().and_then(|x| x.to_str()).unwrap_or("mp3")
    ))
}

fn unique_quarantine_path(dir: &Path, source: &Path) -> PathBuf {
    let filename = source.file_name().unwrap_or_default().to_string_lossy();
    let time = Local::now().format("%Y%m%d-%H%M%S");
    let mut candidate = dir.join(format!("{time}_{filename}"));
    let mut counter = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{time}_{counter}_{filename}"));
        counter += 1;
    }
    candidate
}

fn empty_track() -> Track {
    Track {
        id: 0,
        name: String::new(),
        ar: vec![],
        al: Default::default(),
        dt: 0,
        no: 0,
    }
}
