use anyhow::{anyhow, bail, Context, Result};
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
    pub message: String,
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
    pub errors: Vec<String>,
    pub started_at: String,
    pub finished_at: String,
}

pub async fn sync_one(app: &AppHandle, state: &AppState, playlist_id: u64) -> Result<SyncReport> {
    if state.sync_running.swap(true, Ordering::SeqCst) {
        return Err(anyhow!("已有同步任务正在运行"));
    }
    let _ = app.emit("sync://state", true);
    let result = sync_one_inner(app, state, playlist_id).await;
    state.sync_running.store(false, Ordering::SeqCst);
    let _ = app.emit("sync://state", false);
    if let Ok(report) = &result {
        let _ = app.emit("sync://report", report);
    }
    result
}

pub async fn sync_enabled(app: &AppHandle, state: &AppState) -> Result<Vec<SyncReport>> {
    if state.sync_running.swap(true, Ordering::SeqCst) {
        return Err(anyhow!("已有同步任务正在运行"));
    }
    let _ = app.emit("sync://state", true);
    let config = store::config::load(&state.paths.get().config_file)?;
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
        match sync_one_inner(app, state, id).await {
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

async fn sync_one_inner(app: &AppHandle, state: &AppState, playlist_id: u64) -> Result<SyncReport> {
    state.cancel_requested.store(false, Ordering::SeqCst);
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file)?;
    let root = PathBuf::from(
        config
            .music_root
            .as_deref()
            .context("请先在设置中选择音乐根目录")?,
    );
    let api = NeteaseApi::from_config(&config)?;
    emit(app, playlist_id, "", "读取歌单", 0, 0, "正在拉取歌单曲目");
    let playlist = api.playlist_tracks(playlist_id).await?;
    let setting = config.playlists.iter().find(|p| p.id == playlist_id);
    let quality = setting
        .and_then(|x| x.quality_override.as_deref())
        .unwrap_or(&config.quality);
    let folder_template = setting
        .and_then(|x| x.folder_override.as_deref())
        .unwrap_or(&config.folder_template);
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
    database::log(&conn, &playlist.name, "running", "开始同步")?;

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
        &mut conn,
        &mut report,
    )
    .await?;
    quarantine_removed(
        app,
        &playlist,
        &root,
        folder_template,
        &mut conn,
        &mut report,
    )?;
    if config.write_m3u8 {
        write_m3u8(&playlist, &root, folder_template, &conn)?;
    }

    report.finished_at = database::now();
    let status = if report.failed == 0 { "ok" } else { "error" };
    database::log(
        &conn,
        &playlist.name,
        status,
        &format!(
            "完成：新增 {}，隔离 {}，失败 {}",
            report.added, report.quarantined, report.failed
        ),
    )?;
    database::record_sync_run(&conn, &report)?;
    Ok(report)
}

async fn sync_tracks(
    app: &AppHandle,
    state: &AppState,
    api: &NeteaseApi,
    config: &Config,
    playlist: &PlaylistTracks,
    root: &Path,
    folder_template: &str,
    quality: &str,
    conn: &mut rusqlite::Connection,
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
            return Err(anyhow!("同步已取消"));
        }
        expected.insert(track.id);
        emit(
            app,
            playlist.id,
            &playlist.name,
            "下载",
            index + 1,
            playlist.tracks.len(),
            &track.name,
        );
        match sync_one_track(
            api,
            config,
            playlist,
            root,
            folder_template,
            quality,
            conn,
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
    Failed(String),
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
    conn: &mut rusqlite::Connection,
) {
    if let Err(error) = tags::write_basic_tags(target, track, position, track.id) {
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
    conn: &mut rusqlite::Connection,
    track: &Track,
    position: usize,
    overwrite: bool,
) -> TrackOutcome {
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
            return TrackOutcome::Skipped;
        }
    }

    let song_url = match api.song_url(track.id, quality).await {
        Ok(song_url) => song_url,
        Err(error) => return TrackOutcome::Failed(format!("{}：{error}", track.name)),
    };
    let Some(download_url) = song_url.url else {
        return TrackOutcome::Failed(format!("{}：没有可用下载地址（VIP/版权限制）", track.name));
    };
    let extension = song_url.file_type.as_deref().unwrap_or("mp3");
    let target = naming::track_path(
        root,
        folder_template,
        &config.filename_template,
        &playlist.name,
        track,
        position,
        extension,
    );

    if !overwrite && target.is_file() {
        // 本地已存在按当前模板命名的文件：登记为已同步并跳过下载，避免覆盖用户文件。
        let _ = record_track_file(conn, playlist, track, &target, extension);
        let _ = write_sidecar(&target, playlist.id, track.id);
        let _ = update_snapshot(conn, playlist.id, track.id, position - 1);
        return TrackOutcome::Skipped;
    }

    match download_track(&download_url, &target).await {
        Ok(()) => {
            finalize_track(
                api,
                playlist,
                track,
                position,
                &target,
                extension,
                config.write_lrc,
                conn,
            )
            .await;
            TrackOutcome::Downloaded
        }
        Err(error) => TrackOutcome::Failed(format!("{}：{error}", track.name)),
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
    app: &AppHandle,
    state: &AppState,
    playlist_id: u64,
    track_id: u64,
    options: SingleDownloadOptions,
) -> Result<String> {
    if state.sync_running.load(Ordering::SeqCst) {
        return Err(anyhow!("已有同步任务正在运行，请稍后再试"));
    }
    let paths = state.paths.get();
    let config = store::config::load(&paths.config_file)?;
    let root = PathBuf::from(
        config
            .music_root
            .as_deref()
            .context("请先在设置中选择音乐根目录")?,
    );
    let api = NeteaseApi::from_config(&config)?;
    let playlist = api.playlist_tracks(playlist_id).await?;
    let (index, track) = playlist
        .tracks
        .iter()
        .enumerate()
        .find(|(_, track)| track.id == track_id)
        .context("歌单中不存在该歌曲")?;
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
    let mut conn = database::open(&paths.database_file)?;

    let _ = app.emit(
        "sync://progress",
        SyncProgress {
            playlist_id: Some(playlist_id),
            playlist_name: playlist.name.clone(),
            phase: "下载".into(),
            current: index + 1,
            total: playlist.tracks.len(),
            message: track.name.clone(),
        },
    );

    let song_url = api.song_url(track.id, quality).await?;
    let Some(download_url) = song_url.url else {
        bail!("{}：没有可用下载地址（VIP/版权限制）", track.name);
    };
    let extension = song_url.file_type.as_deref().unwrap_or("mp3");
    let file_name = naming::apply_template(filename_template, &playlist.name, track, index + 1);
    let target = match options.target_dir.as_deref().filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join(format!("{file_name}.{extension}")),
        None => naming::track_path(
            &root,
            folder_template,
            filename_template,
            &playlist.name,
            track,
            index + 1,
            extension,
        ),
    };
    if target.exists() && !options.overwrite {
        return Err(anyhow!(
            "文件已存在：{}，勾选“覆盖已存在”可重新下载",
            target.display()
        ));
    }
    download_track(&download_url, &target).await?;
    finalize_track(
        &api,
        &playlist,
        track,
        index + 1,
        &target,
        extension,
        write_lrc,
        &mut conn,
    )
    .await;
    Ok(target.to_string_lossy().into_owned())
}

async fn download_track(url: &str, target: &Path) -> Result<()> {
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
        return Err(anyhow!("下载内容异常，文件过小"));
    }
    fs::write(&part, bytes)?;
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(part, target)?;
    Ok(())
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

fn quarantine_removed(
    app: &AppHandle,
    playlist: &PlaylistTracks,
    root: &Path,
    folder_template: &str,
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
    ));
    for (track_id, local_path) in stale {
        if ids.contains(&track_id) {
            continue;
        }
        let source = PathBuf::from(&local_path);
        if source.is_file() {
            emit(
                app,
                playlist.id,
                &playlist.name,
                "隔离",
                report.quarantined,
                0,
                source.file_name().and_then(|v| v.to_str()).unwrap_or(""),
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
                    .unwrap_or("未命名"),
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
    conn: &rusqlite::Connection,
) -> Result<()> {
    let folder = root.join(naming::apply_template(
        folder_template,
        &playlist.name,
        &playlist.tracks.first().cloned().unwrap_or_else(empty_track),
        1,
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
    app: &AppHandle,
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
            emit(
                app,
                playlist.id,
                &playlist.name,
                "转换 NCM",
                report.ncm_converted,
                0,
                path.file_name().and_then(|x| x.to_str()).unwrap_or(""),
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
                    report
                        .errors
                        .push(format!("NCM 转换失败 {}：{}", path.display(), error));
                    tracing::warn!(%error, path = %path.display(), "NCM conversion failed");
                }
            }
        }
    }
    Ok(())
}

fn emit(
    app: &AppHandle,
    playlist_id: u64,
    playlist_name: &str,
    phase: &str,
    current: usize,
    total: usize,
    message: &str,
) {
    let _ = app.emit(
        "sync://progress",
        SyncProgress {
            playlist_id: Some(playlist_id),
            playlist_name: playlist_name.to_owned(),
            phase: phase.to_owned(),
            current,
            total,
            message: message.to_owned(),
        },
    );
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
        name: "".into(),
        ar: vec![],
        al: Default::default(),
        dt: 0,
        no: 0,
    }
}
