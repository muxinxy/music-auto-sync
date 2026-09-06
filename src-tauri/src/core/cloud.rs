//! 网易音乐云盘：列出云盘歌曲，并把本地音乐根目录中云盘没有的歌上传到云盘。
//!
//! 上传走“客户端直传”流程（token → 对象存储直传 → complete），不受 API 代理
//! 服务器请求体大小限制（Vercel 4.5MB，FLAC 必挂）；token 返回 needUpload=false
//! 时天然秒传。比对键 = 网易曲目 id：本地文件经旁车/163 key/search-match 解析出
//! id，云盘没有该 id 才上传；无法解析 id 的文件跳过并统计（与补录行为一致）。
//! 引擎复用同步引擎的暂停/取消/进度机制（同一组 AtomicBool 与 sync:// 事件）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

use crate::{
    api::NeteaseApi,
    core::naming,
    error::UiMessage,
    store::{self, database},
    AppState,
};

use super::sync::{
    file_md5, local_file_netease_id, read_local_audio_meta, ui_from_error, SyncErrorDetail,
    SyncProgress, SyncReport,
};

/// 云盘上传任务的哨兵任务名（前端翻译为“云盘上传”）。
pub const CLOUD_TASK_NAME: &str = "cloud";
/// 云盘下载任务的哨兵任务名（前端翻译为“云盘下载”）。
pub const CLOUD_DOWNLOAD_TASK_NAME: &str = "cloud_download";

const CLOUD_PAGE_SIZE: u64 = 200;
/// 分页安全上限：防止 count 异常时无限拉取。
const CLOUD_MAX_ITEMS: u64 = 20_000;
/// 云盘列表聚合缓存 TTL：TTL 内翻页/重复打开不再请求；上传任务结束后主动失效。
const CACHE_TTL_CLOUD: Duration = Duration::from_secs(120);

/// 云盘任务的暂停/取消标志对（独立于歌单同步，二者可并行）。
type CloudCtl<'a> = (&'a AtomicBool, &'a AtomicBool);

fn cloud_is_canceled(ctl: CloudCtl<'_>) -> bool {
    ctl.1.load(Ordering::SeqCst)
}

async fn cloud_wait_if_paused(ctl: CloudCtl<'_>) -> Result<(), UiMessage> {
    while ctl.0.load(Ordering::SeqCst) {
        if cloud_is_canceled(ctl) {
            return Err(UiMessage::new("syncCanceled"));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    Ok(())
}

/// 云盘任务进度（独立事件通道 cloud://progress，与歌单同步并行互不干扰）。
#[allow(clippy::too_many_arguments)]
fn emit_cloud_progress(
    app: Option<&AppHandle>,
    playlist_name: &str,
    phase: &str,
    current: usize,
    total: usize,
    message: UiMessage,
) {
    if let Some(app) = app {
        let _ = app.emit(
            "cloud://progress",
            SyncProgress {
                playlist_id: None,
                playlist_name: playlist_name.to_owned(),
                phase: phase.to_owned(),
                current,
                total,
                message,
                run_id: None,
            },
        );
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSong {
    /// 云盘条目 songId（与匹配到的网易曲目 id 一致；未匹配为 0）。
    pub song_id: u64,
    pub song_name: String,
    pub artist: String,
    pub album: String,
    pub file_name: String,
    pub file_size: u64,
    pub bitrate: Option<u64>,
    /// 上传时间（epoch 毫秒）。
    pub add_time: Option<u64>,
    /// simpleSong.id：匹配到的网易曲目 id（未匹配为 0/缺失）。
    pub simple_song_id: Option<u64>,
}

impl CloudSong {
    /// 与本地匹配结果比对用的网易曲目 id。
    fn matched_id(&self) -> Option<u64> {
        self.simple_song_id
            .filter(|id| *id > 0)
            .or((self.song_id > 0).then_some(self.song_id))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudStorageInfo {
    pub count: u64,
    pub used_size: Option<u64>,
    pub max_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudListResult {
    pub songs: Vec<CloudSong>,
    pub storage: CloudStorageInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudUploadItem {
    pub path: String,
    pub file_name: String,
    pub file_size: u64,
    pub netease_id: u64,
    /// 来自本地标签的元数据，complete 时回传给网易；缺失留空由服务端补默认值。
    pub title: String,
    pub artist: String,
    pub album: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloudUploadPlan {
    pub uploads: Vec<CloudUploadItem>,
    /// 无法匹配网易曲目而将被跳过的本地文件路径。
    pub unresolved: Vec<String>,
    pub cloud_count: usize,
    pub local_count: usize,
}

/// 拉取云盘全量列表（分页 200/页）。聚合结果整体进共享缓存（TTL 120s），
/// `force=true` 跳过缓存直拉（同步路径/手动刷新用）。
pub async fn fetch_cloud_list(api: &NeteaseApi, force: bool) -> Result<CloudListResult> {
    let cached = if force {
        None
    } else {
        api.cache_get("user_cloud", "all", CACHE_TTL_CLOUD)
    };
    let (items, count, used_size, max_size): (Vec<Value>, u64, Option<u64>, Option<u64>) =
        match cached {
            Some(value) => (
                value
                    .get("data")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                value.get("count").and_then(Value::as_u64).unwrap_or(0),
                value.get("size").and_then(Value::as_u64),
                value.get("maxSize").and_then(Value::as_u64),
            ),
            None => {
                let mut items: Vec<Value> = Vec::new();
                let mut count = 0u64;
                let mut used_size = None;
                let mut max_size = None;
                let mut offset = 0u64;
                loop {
                    let (page_items, page_count, size, cap) =
                        api.user_cloud_page(CLOUD_PAGE_SIZE, offset).await?;
                    if offset == 0 {
                        count = page_count;
                        used_size = size;
                        max_size = cap;
                    }
                    let page_len = page_items.len() as u64;
                    items.extend(page_items);
                    offset += page_len;
                    if page_len == 0
                        || page_len < CLOUD_PAGE_SIZE
                        || offset >= count
                        || offset >= CLOUD_MAX_ITEMS
                    {
                        break;
                    }
                }
                api.cache_put(
                    "user_cloud",
                    "all",
                    &serde_json::json!({
                        "data": &items,
                        "count": count,
                        "size": used_size,
                        "maxSize": max_size
                    }),
                );
                (items, count, used_size, max_size)
            }
        };
    let songs = items.iter().map(parse_cloud_song).collect();
    Ok(CloudListResult {
        storage: CloudStorageInfo {
            count,
            used_size,
            max_size,
        },
        songs,
    })
}

/// 从 /user/cloud 单条数据解析云盘歌曲。字段逐级回退：
/// songName → simpleSong.name → fileName。
fn parse_cloud_song(item: &serde_json::Value) -> CloudSong {
    let simple = item.get("simpleSong");
    let file_name = json_str(item.get("fileName")).unwrap_or_default();
    let simple_song_id = simple
        .and_then(|song| song.get("id"))
        .and_then(serde_json::Value::as_u64)
        .filter(|id| *id > 0);
    let song_id = item
        .get("songId")
        .and_then(serde_json::Value::as_u64)
        .or(simple_song_id)
        .unwrap_or(0);
    let song_name = json_str(item.get("songName"))
        .or_else(|| simple.and_then(|song| json_str(song.get("name"))))
        .unwrap_or_else(|| file_name.clone());
    CloudSong {
        song_id,
        song_name,
        artist: json_str(item.get("artist")).unwrap_or_default(),
        album: json_str(item.get("album")).unwrap_or_default(),
        file_name,
        file_size: item
            .get("fileSize")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        bitrate: item.get("bitrate").and_then(serde_json::Value::as_u64),
        add_time: item.get("addTime").and_then(serde_json::Value::as_u64),
        simple_song_id,
    }
}

fn json_str(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// 云盘任务对比/上传明细行：每对比一首本地文件发一条（事件 `cloud://row`，
/// 前端按 path 合并成实时明细表）。result 取值：
/// in_cloud / duplicate / unresolved / to_upload / uploading / uploaded / instant / failed
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTaskRow {
    pub path: String,
    pub file_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub netease_id: u64,
    pub file_size: u64,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<UiMessage>,
}

fn emit_task_row(app: Option<&AppHandle>, row: CloudTaskRow) {
    if let Some(app) = app {
        let _ = app.emit("cloud://row", &row);
    }
}

fn upload_row(item: &CloudUploadItem, result: &str, message: Option<UiMessage>) -> CloudTaskRow {
    CloudTaskRow {
        path: item.path.clone(),
        file_name: item.file_name.clone(),
        title: item.title.clone(),
        artist: item.artist.clone(),
        album: item.album.clone(),
        netease_id: item.netease_id,
        file_size: item.file_size,
        result: result.to_string(),
        message,
    }
}

/// 判断一个已匹配的本地 id 的对比结果（seen 记录本次已出现过的 id 用于去重）。
fn classify_match(cloud_ids: &HashSet<u64>, seen: &mut HashSet<u64>, id: u64) -> &'static str {
    if cloud_ids.contains(&id) {
        return "in_cloud";
    }
    if seen.insert(id) {
        "to_upload"
    } else {
        "duplicate"
    }
}

/// 云盘任务的数据来源：自动扫描音乐根目录比对；或用户手动选择的文件/目录。
pub enum CloudSource {
    AutoScan,
    ManualPaths(Vec<String>),
}

/// 音频文件判定：扩展名白名单 + 跳过隔离目录内文件。
fn is_audio_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|x| x.to_str()) else {
        return false;
    };
    if !matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp3" | "flac" | "m4a" | "wav" | "ogg" | "aac"
    ) {
        return false;
    }
    !path.to_string_lossy().contains(".quarantine")
}

/// 扫描音乐根目录 → 解析网易 id → 与云盘比对，产出上传计划。
/// 只读不改云盘；`app` 传入时发扫描进度，`controls` 传入时响应暂停/取消。
pub async fn build_upload_plan(
    app: Option<&AppHandle>,
    controls: Option<CloudCtl<'_>>,
    api: &NeteaseApi,
    root: &Path,
) -> Result<CloudUploadPlan> {
    // 同步路径强制拉最新云盘列表（缓存可能落后于刚完成的任务）。
    let cloud = fetch_cloud_list(api, true).await?;
    let cloud_ids: HashSet<u64> = cloud.songs.iter().filter_map(|s| s.matched_id()).collect();

    emit_cloud_progress(
        app,
        CLOUD_TASK_NAME,
        "phase_scan_local",
        0,
        0,
        UiMessage::new("scanLocalFiles"),
    );

    let mut files: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_file() && is_audio_file(path) {
            files.push(path.to_path_buf());
        }
    }

    let total = files.len();
    let mut plan = CloudUploadPlan {
        cloud_count: cloud.songs.len(),
        local_count: total,
        ..Default::default()
    };
    let mut seen: HashSet<u64> = HashSet::new();
    for (index, path) in files.iter().enumerate() {
        if let Some(ctl) = controls {
            cloud_wait_if_paused(ctl).await?;
            if cloud_is_canceled(ctl) {
                return Err(anyhow!(UiMessage::new("syncCanceled")));
            }
        }
        let display_path = path.to_string_lossy().into_owned();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        match local_file_netease_id(api, path).await {
            Ok(Some(id)) => {
                let result = classify_match(&cloud_ids, &mut seen, id);
                let (title, artist, album) = read_local_audio_meta(path)
                    .ok()
                    .flatten()
                    .map(|(title, artist, album, _)| (title, artist, album))
                    .unwrap_or_default();
                let row = CloudTaskRow {
                    path: display_path.clone(),
                    file_name: file_name.clone(),
                    title: title.clone(),
                    artist: artist.clone(),
                    album: album.clone(),
                    netease_id: id,
                    file_size,
                    result: result.to_string(),
                    message: None,
                };
                if result == "to_upload" {
                    plan.uploads.push(CloudUploadItem {
                        path: display_path,
                        file_name,
                        file_size,
                        netease_id: id,
                        title,
                        artist,
                        album,
                    });
                }
                emit_task_row(app, row);
            }
            Ok(None) => {
                plan.unresolved.push(display_path.clone());
                emit_task_row(
                    app,
                    CloudTaskRow {
                        path: display_path,
                        file_name,
                        title: String::new(),
                        artist: String::new(),
                        album: String::new(),
                        netease_id: 0,
                        file_size,
                        result: "unresolved".to_string(),
                        message: None,
                    },
                );
            }
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "cloud upload: local file resolve failed");
                plan.unresolved.push(display_path.clone());
                emit_task_row(
                    app,
                    CloudTaskRow {
                        path: display_path,
                        file_name,
                        title: String::new(),
                        artist: String::new(),
                        album: String::new(),
                        netease_id: 0,
                        file_size,
                        result: "unresolved".to_string(),
                        message: Some(error.downcast_ref::<UiMessage>().cloned().unwrap_or_else(|| UiMessage::unknown(error))),
                    },
                );
            }
        }
        emit_cloud_progress(
            app,
            CLOUD_TASK_NAME,
            "phase_scan_local",
            index + 1,
            total,
            UiMessage::new("scanLocalFiles"),
        );
    }
    Ok(plan)
}

/// 手动上传计划：用户选择的文件/目录（目录递归扫描音频）。用户明确指定即上传
/// （含无法匹配网易曲目的文件），仅跳过云盘已有同曲目的文件。
pub async fn build_manual_plan(
    app: Option<&AppHandle>,
    controls: Option<CloudCtl<'_>>,
    api: &NeteaseApi,
    paths: &[String],
) -> Result<CloudUploadPlan> {
    let cloud = fetch_cloud_list(api, true).await?;
    let cloud_ids: HashSet<u64> = cloud.songs.iter().filter_map(|s| s.matched_id()).collect();

    let mut files: Vec<PathBuf> = Vec::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    for raw in paths {
        let p = PathBuf::from(raw);
        if p.is_dir() {
            for entry in WalkDir::new(&p)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
            {
                let path = entry.path();
                if path.is_file() && is_audio_file(path) && seen_paths.insert(path.to_path_buf()) {
                    files.push(path.to_path_buf());
                }
            }
        } else if p.is_file() && is_audio_file(&p) && seen_paths.insert(p.clone()) {
            files.push(p);
        }
    }

    let total = files.len();
    let mut plan = CloudUploadPlan {
        cloud_count: cloud.songs.len(),
        local_count: total,
        ..Default::default()
    };
    emit_cloud_progress(
        app,
        CLOUD_TASK_NAME,
        "phase_scan_local",
        0,
        total,
        UiMessage::new("scanLocalFiles"),
    );
    for (index, path) in files.iter().enumerate() {
        if let Some(ctl) = controls {
            cloud_wait_if_paused(ctl).await?;
            if cloud_is_canceled(ctl) {
                return Err(anyhow!(UiMessage::new("syncCanceled")));
            }
        }
        let display_path = path.to_string_lossy().into_owned();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let id = local_file_netease_id(api, path).await.unwrap_or(None);
        let in_cloud = id.is_some_and(|id| cloud_ids.contains(&id));
        let (title, artist, album) = read_local_audio_meta(path)
            .ok()
            .flatten()
            .map(|(title, artist, album, _)| (title, artist, album))
            .unwrap_or_default();
        let row = CloudTaskRow {
            path: display_path.clone(),
            file_name: file_name.clone(),
            title: title.clone(),
            artist: artist.clone(),
            album: album.clone(),
            netease_id: id.unwrap_or(0),
            file_size,
            result: if in_cloud { "in_cloud" } else { "to_upload" }.to_string(),
            message: None,
        };
        if !in_cloud {
            plan.uploads.push(CloudUploadItem {
                path: display_path,
                file_name,
                file_size,
                netease_id: id.unwrap_or(0),
                title,
                artist,
                album,
            });
        }
        emit_task_row(app, row);
        emit_cloud_progress(
            app,
            CLOUD_TASK_NAME,
            "phase_scan_local",
            index + 1,
            total,
            UiMessage::new("scanLocalFiles"),
        );
    }
    Ok(plan)
}

/// 云盘上传入口：独立于歌单同步的互斥（cloud_running），二者可并行；
/// 事件走 cloud:// 命名通道；报告在任何结局（完成/取消/失败）都会发出。
pub async fn sync_cloud(
    app: &AppHandle,
    state: &AppState,
    source: CloudSource,
) -> Result<SyncReport, UiMessage> {
    if state.cloud_running.swap(true, Ordering::SeqCst) {
        return Err(UiMessage::new("syncBusy"));
    }
    let _ = app.emit("cloud://state", true);
    let (report, run_error) = sync_cloud_inner(Some(app), state, &source).await;
    state.cloud_running.store(false, Ordering::SeqCst);
    state.cloud_pause_requested.store(false, Ordering::SeqCst);
    let _ = app.emit("cloud://state", false);
    let _ = app.emit("cloud://report", &report);
    // 云盘列表缓存已过期（有上传/秒传变更），让 UI 下次读取拿最新。
    state.api_cache.invalidate_namespace("user_cloud");
    match run_error {
        Some(message) => Err(message),
        None => Ok(report),
    }
}

/// 云盘上传主流程。**任何结局（完成/取消/中途失败）都会写 sync_logs、record_sync_run
/// 并把 report 返回给调用方**，错误通过第二个返回值表达（report 里也带已累计的计数）。
async fn sync_cloud_inner(
    app: Option<&AppHandle>,
    state: &AppState,
    source: &CloudSource,
) -> (SyncReport, Option<UiMessage>) {
    state.cloud_cancel_requested.store(false, Ordering::SeqCst);
    state.cloud_pause_requested.store(false, Ordering::SeqCst);
    let paths = state.paths.get();
    let mut run_error: Option<UiMessage> = None;
    let mut report = empty_cloud_report(CLOUD_TASK_NAME);

    // 日志先行：保证任何结局都有 "running" 记录可收尾（DB 打不开是唯一无日志的路径）。
    let mut conn = match database::open(&paths.database_file) {
        Ok(conn) => conn,
        Err(error) => {
            report.finished_at = database::now();
            return (report, Some(ui_from_error(error)));
        }
    };
    let sync_run_id = database::log(
        &conn,
        CLOUD_TASK_NAME,
        "running",
        &UiMessage::new("syncStart").to_json(),
    )
    .unwrap_or(0);
    // 明细表换轮：前端收到 cloud://start（携带本轮 run id）清空上一轮明细；
    // 同步日志的任务详情对最近一次云盘任务直接渲染内存明细（实时且含比对记录）。
    if let Some(app) = app {
        let _ = app.emit("cloud://start", sync_run_id);
    }

    // 主流程：任何一步失败（含取消）都不提前返回，错误经 run_error 统一收尾。
    let outcome: Result<()> = async {
        let config = store::config::load(&paths.config_file)?;
        let api = NeteaseApi::from_config(&config)?;
        let ctl: CloudCtl<'_> = (&state.cloud_pause_requested, &state.cloud_cancel_requested);

        let plan = match source {
            CloudSource::AutoScan => {
                // 自动比对需要音乐根目录。
                let root = config
                    .music_root
                    .as_deref()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!(UiMessage::new("musicRootRequired")))?;
                build_upload_plan(app, Some(ctl), &api, &root).await?
            }
            CloudSource::ManualPaths(paths) => {
                build_manual_plan(app, Some(ctl), &api, paths).await?
            }
        };
        if !plan.unresolved.is_empty() {
            report.errors.push(UiMessage::with_params(
                "localUnresolvedTracks",
                vec![plan.unresolved.len().to_string()],
            ));
        }

        let uploads = Arc::new(plan.uploads);
        let total = uploads.len();
        // 上传并发沿用下载并发配置（桌面场景带宽与磁盘 IO 同源）。
        let concurrency = config.concurrency.max(1);
        let upload_client = NeteaseApi::build_upload_client(&config)?;
        emit_cloud_progress(
            app,
            CLOUD_TASK_NAME,
            "phase_upload_cloud",
            0,
            total,
            UiMessage::with_params(
                "track",
                uploads.first().map_or_else(Vec::new, |item| {
                    vec![item.file_name.clone()]
                }),
            ),
        );

        // 滑动窗口并发（JoinSet），曲目/文件边界检查暂停与取消——绝不退回“全 spawn 再 join”。
        let mut next_index = 0usize;
        let mut done = 0usize;
        let mut running = tokio::task::JoinSet::new();
        while next_index < total || !running.is_empty() {
            if let Err(error) = cloud_wait_if_paused(ctl).await {
                running.abort_all();
                anyhow::bail!(error);
            }
            if cloud_is_canceled(ctl) {
                running.abort_all();
                anyhow::bail!(UiMessage::new("syncCanceled"));
            }
            while next_index < total && running.len() < concurrency {
                let index = next_index;
                next_index += 1;
                let item = uploads[index].clone();
                let api = api.clone();
                let client = upload_client.clone();
                running.spawn(async move {
                    let result = upload_one(&api, &client, &item).await;
                    (index, result)
                });
                emit_task_row(app, upload_row(&uploads[index], "uploading", None));
            }
            let joined = running.join_next().await.transpose();
            let (index, result) = match joined {
                Ok(Some(pair)) => pair,
                Ok(None) => continue,
                Err(join_error) => {
                    running.abort_all();
                    return Err(anyhow!(UiMessage::unknown(join_error)));
                }
            };
            done += 1;
            let file_name = uploads[index].file_name.clone();
            let netease_id = uploads[index].netease_id;
            match result {
                Ok(instant) => {
                    let action = if instant { "instant_import" } else { "added_cloud" };
                    let row_result = if instant { "instant" } else { "uploaded" };
                    if instant {
                        report.skipped += 1;
                    } else {
                        report.added += 1;
                    }
                    let _ = database::record_change(
                        &mut conn,
                        sync_run_id,
                        0,
                        CLOUD_TASK_NAME,
                        "to_cloud",
                        action,
                        Some(netease_id),
                        Some(&file_name),
                        Some(&uploads[index].path),
                        None,
                        Some(netease_id),
                        None,
                    );
                    emit_task_row(app, upload_row(&uploads[index], row_result, None));
                }
                Err(error) => {
                    let message = ui_from_error(error);
                    report.failed += 1;
                    report.error_details.push(SyncErrorDetail {
                        track_id: netease_id,
                        track_name: file_name.clone(),
                        message: message.clone(),
                    });
                    report.errors.push(message.clone());
                    // 失败也记入变更流水（action=failed，note=错误 UiMessage），
                    // 任务详情里能看到每首的失败原因。
                    let _ = database::record_change(
                        &mut conn,
                        sync_run_id,
                        0,
                        CLOUD_TASK_NAME,
                        "to_cloud",
                        "failed",
                        Some(netease_id),
                        Some(&file_name),
                        Some(&uploads[index].path),
                        None,
                        Some(netease_id),
                        Some(&message.to_json()),
                    );
                    emit_task_row(app, upload_row(&uploads[index], "failed", Some(message)));
                }
            }
            emit_cloud_progress(
                app,
                CLOUD_TASK_NAME,
                "phase_upload_cloud",
                done,
                total,
                UiMessage::with_params("track", vec![file_name]),
            );
        }
        Ok(())
    }
    .await;
    if let Err(error) = outcome {
        run_error = Some(ui_from_error(error));
    }

    // 统一收尾：无论完成/取消/失败都写日志与同步记录，并把错误带回报告。
    if let Some(error) = &run_error {
        report.errors.push(error.clone());
        report.error_details.push(SyncErrorDetail {
            track_id: 0,
            track_name: CLOUD_TASK_NAME.to_owned(),
            message: error.clone(),
        });
    }
    report.finished_at = database::now();
    let status = match &run_error {
        Some(error) if error.code == "syncCanceled" => "canceled",
        Some(_) => "error",
        None if report.failed == 0 => "ok",
        None => "error",
    };
    // 日志正文：失败/取消写明原因（如未设音乐根目录），成功写计数摘要。
    let log_message = match &run_error {
        Some(error) => error.to_json(),
        None => UiMessage::with_params(
            "syncDone",
            vec![
                report.added.to_string(),
                report.quarantined.to_string(),
                report.failed.to_string(),
            ],
        )
        .to_json(),
    };
    if let Err(error) = database::finish_log(&conn, sync_run_id, status, &log_message) {
        tracing::warn!(%error, "cloud sync: finalize log failed");
    }
    if let Err(error) = database::record_sync_run(&conn, &report) {
        tracing::warn!(%error, "cloud sync: record sync run failed");
    }
    (report, run_error)
}

fn empty_cloud_report(playlist_name: &str) -> SyncReport {
    SyncReport {
        playlist_id: 0,
        playlist_name: playlist_name.to_owned(),
        added: 0,
        updated: 0,
        quarantined: 0,
        ncm_converted: 0,
        failed: 0,
        skipped: 0,
        errors: vec![],
        error_details: vec![],
        started_at: database::now(),
        finished_at: String::new(),
    }
}

/// 单文件上传：md5 → token →（需要时）对象存储直传 → complete。
/// 返回 true 表示秒传（服务器已有同 MD5 文件，未实际传输）。
async fn upload_one(
    api: &NeteaseApi,
    client: &reqwest::Client,
    item: &CloudUploadItem,
) -> Result<bool> {
    let path = Path::new(&item.path);
    let md5 = file_md5(path);
    if md5.is_empty() {
        return Err(anyhow!(UiMessage::with_params(
            "cloudFileReadFailed",
            vec![item.path.clone(), "无法读取文件内容".to_owned()]
        )));
    }
    let ticket = api
        .cloud_upload_ticket(&md5, item.file_size, &item.file_name)
        .await
        .map_err(|error| {
            anyhow!(UiMessage::with_params(
                "cloudTokenFailed",
                vec![item.file_name.clone(), error.to_string()]
            ))
        })?;
    if ticket.need_upload {
        NeteaseApi::cloud_transfer_file(client, &ticket, path, &md5).await?;
    }
    api.cloud_upload_complete(
        &ticket,
        &md5,
        &item.file_name,
        Some(&item.title),
        Some(&item.artist),
        Some(&item.album),
    )
    .await
    .map_err(|error| {
        anyhow!(UiMessage::with_params(
            "cloudCompleteFailed",
            vec![item.file_name.clone(), error.to_string()]
        ))
    })?;
    Ok(!ticket.need_upload)
}

/// 云盘歌曲下载任务的单项输入（命令参数）。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudDownloadItem {
    pub id: u64,
    pub file_name: String,
}

/// 多选下载云盘歌曲到指定目录：与云盘上传共用任务机制（cloud_running 互斥、
/// cloud:// 事件、同步日志一条并在结束时原地更新、可暂停/取消），逐首串行下载。
/// 每首都记入变更流水（downloaded_cloud / skipped_exists / failed），任务详情可查。
pub async fn download_cloud_items(
    app: &AppHandle,
    state: &AppState,
    items: Vec<CloudDownloadItem>,
    target_dir: PathBuf,
) -> Result<SyncReport, UiMessage> {
    if state.cloud_running.swap(true, Ordering::SeqCst) {
        return Err(UiMessage::new("syncBusy"));
    }
    let _ = app.emit("cloud://state", true);
    let (report, run_error) = download_items_inner(Some(app), state, items, target_dir).await;
    state.cloud_running.store(false, Ordering::SeqCst);
    state.cloud_pause_requested.store(false, Ordering::SeqCst);
    let _ = app.emit("cloud://state", false);
    let _ = app.emit("cloud://report", &report);
    match run_error {
        Some(message) => Err(message),
        None => Ok(report),
    }
}

async fn download_items_inner(
    app: Option<&AppHandle>,
    state: &AppState,
    items: Vec<CloudDownloadItem>,
    target_dir: PathBuf,
) -> (SyncReport, Option<UiMessage>) {
    state.cloud_cancel_requested.store(false, Ordering::SeqCst);
    state.cloud_pause_requested.store(false, Ordering::SeqCst);
    let paths = state.paths.get();
    let mut run_error: Option<UiMessage> = None;
    let mut report = empty_cloud_report(CLOUD_DOWNLOAD_TASK_NAME);

    let mut conn = match database::open(&paths.database_file) {
        Ok(conn) => conn,
        Err(error) => {
            report.finished_at = database::now();
            return (report, Some(ui_from_error(error)));
        }
    };
    let sync_run_id = database::log(
        &conn,
        CLOUD_DOWNLOAD_TASK_NAME,
        "running",
        &UiMessage::new("syncStart").to_json(),
    )
    .unwrap_or(0);
    if let Some(app) = app {
        let _ = app.emit("cloud://start", sync_run_id);
    }

    let outcome: Result<()> = async {
        let config = store::config::load(&paths.config_file)?;
        let api = NeteaseApi::from_config(&config)?;
        let client = NeteaseApi::build_upload_client(&config)?;
        tokio::fs::create_dir_all(&target_dir)
            .await
            .map_err(|error| {
                anyhow!(UiMessage::with_params("downloadFailed", vec![error.to_string()]))
            })?;

        let ctl: CloudCtl<'_> = (&state.cloud_pause_requested, &state.cloud_cancel_requested);
        let total = items.len();
        for (index, item) in items.iter().enumerate() {
            cloud_wait_if_paused(ctl).await?;
            if cloud_is_canceled(ctl) {
                anyhow::bail!(UiMessage::new("syncCanceled"));
            }
            emit_cloud_progress(
                app,
                CLOUD_DOWNLOAD_TASK_NAME,
                "phase_download_cloud",
                index,
                total,
                UiMessage::with_params("track", vec![item.file_name.clone()]),
            );
            emit_task_row(
                app,
                CloudTaskRow {
                    path: format!("cloud-dl://{}", item.id),
                    file_name: item.file_name.clone(),
                    title: String::new(),
                    artist: String::new(),
                    album: String::new(),
                    netease_id: item.id,
                    file_size: 0,
                    result: "downloading".to_string(),
                    message: None,
                },
            );

            let target = target_dir.join(naming::sanitize_component(&item.file_name));
            let action: Result<&'static str> = async {
                if target.exists() {
                    return Ok("skipped_exists");
                }
                let (url, _size) = api.cloud_download_url(item.id).await?;
                let mut response = client.get(&url).send().await.map_err(|error| {
                    anyhow!(UiMessage::with_params(
                        "downloadFailed",
                        vec![error.to_string()]
                    ))
                })?;
                if !response.status().is_success() {
                    return Err(anyhow!(UiMessage::with_params(
                        "downloadFailed",
                        vec![format!("HTTP {}", response.status().as_u16())]
                    )));
                }
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::File::create(&target).await.map_err(|error| {
                    anyhow!(UiMessage::with_params(
                        "downloadFailed",
                        vec![error.to_string()]
                    ))
                })?;
                while let Some(chunk) = response.chunk().await.map_err(|error| {
                    anyhow!(UiMessage::with_params(
                        "downloadFailed",
                        vec![error.to_string()]
                    ))
                })? {
                    file.write_all(&chunk).await.map_err(|error| {
                        anyhow!(UiMessage::with_params(
                            "downloadFailed",
                            vec![error.to_string()]
                        ))
                    })?;
                }
                file.flush().await.map_err(|error| {
                    anyhow!(UiMessage::with_params(
                        "downloadFailed",
                        vec![error.to_string()]
                    ))
                })?;
                Ok("downloaded_cloud")
            }
            .await;

            let row_path = format!("cloud-dl://{}", item.id);
            match action {
                Ok(action) => {
                    if action == "downloaded_cloud" {
                        report.added += 1;
                    } else {
                        report.skipped += 1;
                    }
                    let _ = database::record_change(
                        &mut conn,
                        sync_run_id,
                        0,
                        CLOUD_DOWNLOAD_TASK_NAME,
                        "from_cloud",
                        action,
                        Some(item.id),
                        Some(&item.file_name),
                        Some(&target.to_string_lossy()),
                        None,
                        Some(item.id),
                        None,
                    );
                    emit_task_row(
                        app,
                        CloudTaskRow {
                            path: row_path,
                            file_name: item.file_name.clone(),
                            title: String::new(),
                            artist: String::new(),
                            album: String::new(),
                            netease_id: item.id,
                            file_size: 0,
                            result: if action == "downloaded_cloud" {
                                "downloaded"
                            } else {
                                "dl_skipped"
                            }
                            .to_string(),
                            message: None,
                        },
                    );
                }
                Err(error) => {
                    let message = ui_from_error(error);
                    report.failed += 1;
                    report.error_details.push(SyncErrorDetail {
                        track_id: item.id,
                        track_name: item.file_name.clone(),
                        message: message.clone(),
                    });
                    report.errors.push(message.clone());
                    let _ = database::record_change(
                        &mut conn,
                        sync_run_id,
                        0,
                        CLOUD_DOWNLOAD_TASK_NAME,
                        "from_cloud",
                        "failed",
                        Some(item.id),
                        Some(&item.file_name),
                        Some(&target.to_string_lossy()),
                        None,
                        Some(item.id),
                        Some(&message.to_json()),
                    );
                    emit_task_row(
                        app,
                        CloudTaskRow {
                            path: row_path,
                            file_name: item.file_name.clone(),
                            title: String::new(),
                            artist: String::new(),
                            album: String::new(),
                            netease_id: item.id,
                            file_size: 0,
                            result: "failed".to_string(),
                            message: Some(message),
                        },
                    );
                }
            }
            emit_cloud_progress(
                app,
                CLOUD_DOWNLOAD_TASK_NAME,
                "phase_download_cloud",
                index + 1,
                total,
                UiMessage::with_params("track", vec![item.file_name.clone()]),
            );
        }
        Ok(())
    }
    .await;
    if let Err(error) = outcome {
        run_error = Some(ui_from_error(error));
    }

    report.finished_at = database::now();
    let status = match &run_error {
        Some(error) if error.code == "syncCanceled" => "canceled",
        Some(_) => "error",
        None if report.failed == 0 => "ok",
        None => "error",
    };
    let log_message = match &run_error {
        Some(error) => error.to_json(),
        None => UiMessage::with_params(
            "syncDone",
            vec![
                report.added.to_string(),
                report.quarantined.to_string(),
                report.failed.to_string(),
            ],
        )
        .to_json(),
    };
    if let Err(error) = database::finish_log(&conn, sync_run_id, status, &log_message) {
        tracing::warn!(%error, "cloud download: finalize log failed");
    }
    if let Err(error) = database::record_sync_run(&conn, &report) {
        tracing::warn!(%error, "cloud download: record sync run failed");
    }
    (report, run_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_cloud_song_item() {
        let item = json!({
            "privateCloud": { "id": 435055185451i64, "songId": 30284138 },
            "songId": 30284138,
            "songName": "人民海军向前进",
            "artist": "霍勇",
            "album": "我是一个兵",
            "fileName": "霍勇 - 人民海军向前进.mp3",
            "fileSize": 3263723,
            "bitrate": 327,
            "addTime": 1643624375390i64,
            "simpleSong": { "id": 30284138, "name": "人民海军向前进" }
        });
        let song = parse_cloud_song(&item);
        assert_eq!(song.song_id, 30284138);
        assert_eq!(song.simple_song_id, Some(30284138));
        assert_eq!(song.matched_id(), Some(30284138));
        assert_eq!(song.song_name, "人民海军向前进");
        assert_eq!(song.artist, "霍勇");
        assert_eq!(song.file_size, 3263723);
        assert_eq!(song.bitrate, Some(327));
        assert_eq!(song.add_time, Some(1643624375390));
        assert_eq!(song.file_name, "霍勇 - 人民海军向前进.mp3");
    }

    #[test]
    fn falls_back_when_fields_missing() {
        // 未匹配条目：simpleSong.id=0 视为未匹配；歌名回退到 simpleSong.name。
        let item = json!({
            "songId": 0,
            "fileName": "raw name.flac",
            "fileSize": 10,
            "simpleSong": { "id": 0, "name": "song title" }
        });
        let song = parse_cloud_song(&item);
        assert_eq!(song.song_id, 0);
        assert_eq!(song.simple_song_id, None);
        assert_eq!(song.matched_id(), None);
        assert_eq!(song.song_name, "song title");
        assert_eq!(song.file_name, "raw name.flac");
    }

    #[test]
    fn tolerates_completely_empty_item() {
        let song = parse_cloud_song(&json!({}));
        assert_eq!(song.song_id, 0);
        assert_eq!(song.song_name, "");
        assert_eq!(song.file_size, 0);
    }

    #[test]
    fn classify_match_distinguishes_cloud_dup_and_upload() {
        let cloud_ids: HashSet<u64> = [1u64, 2].into_iter().collect();
        let mut seen = HashSet::new();
        // 云盘已有 → in_cloud。
        assert_eq!(classify_match(&cloud_ids, &mut seen, 1), "in_cloud");
        // 云盘没有 → 待上传，且第二次出现同 id 归为重复。
        assert_eq!(classify_match(&cloud_ids, &mut seen, 3), "to_upload");
        assert_eq!(classify_match(&cloud_ids, &mut seen, 3), "duplicate");
    }
}
