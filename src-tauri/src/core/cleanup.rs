//! 重复文件清理工具：按网易曲目 id 分组找重复，按音质分组、按标签完整度与
//! 修改时间决定保留项。清理动作一律移入隔离区（可在隔离区页恢复）。
//!
//! 分组与保留规则：
//!  - 曲目识别：`local_file_netease_id`（旁车 → 163 key → 联网 tag 匹配）；
//!    无法识别的文件不参与清理（单列展示，action=keep）。
//!  - 保留多音质：同曲先按音质键分组，各组内保留一个；否则同曲只留一个。
//!  - 各组内保留排序：属性得分高者优先，其次修改时间新者优先。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use chrono::Local;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};
use serde::Serialize;
use walkdir::WalkDir;

use crate::api::NeteaseApi;
use crate::core::sync::local_file_netease_id;

/// 与同步引擎一致的音频扩展名（sync.rs 同款集合）。
pub(crate) const AUDIO_EXTS: [&str; 6] = ["mp3", "flac", "m4a", "wav", "ogg", "aac"];

/// 清理工具在隔离区表里的来源哨兵（playlist_name 字段），前端据此翻译显示。
pub const CLEANUP_SOURCE: &str = "cleanup";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupFileItem {
    pub path: String,
    /// 展示用音质名（即扩展名大写，如 "MP3"/"FLAC"）。
    pub quality: String,
    /// 分组用音质键（扩展名小写）。
    pub quality_key: String,
    /// 音频码率 bps（读不出为 0）；同后缀内比"最高音质"用。
    pub bitrate: u32,
    pub size: u64,
    pub modified_at: String,
    /// 属性完整度得分（每项属性 1 分）。
    pub score: u32,
    pub has_title: bool,
    pub has_artist: bool,
    pub has_album: bool,
    pub has_cover: bool,
    pub has_lyrics: bool,
    pub has_comment: bool,
    pub has_sidecar: bool,
    pub action: String, // keep | clean
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupGroup {
    /// 展示用曲名（取保留项的标签，退化为文件名）。
    pub song: String,
    pub netease_id: Option<u64>,
    pub items: Vec<CleanupFileItem>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupScanReport {
    pub scanned_files: usize,
    pub identified: usize,
    pub unresolved: usize,
    /// 含 ≥2 个文件的重复组数。
    pub duplicate_groups: usize,
    pub clean_count: usize,
    pub clean_bytes: u64,
    pub groups: Vec<CleanupGroup>,
}

/// 单个文件的身份与标签信息。
struct Entry {
    path: PathBuf,
    netease_id: Option<u64>,
    item: CleanupFileItem,
    title: String,
    artist: String,
}

/// 扫描目录找出重复组。识别可能联网（无旁车/无 163 key 的文件走 /search/match）。
/// `on_progress(completed, total, current_file)` 随识别进度回调。
/// 返回 None 表示被取消。
pub async fn scan(
    api: &NeteaseApi,
    dirs: &[String],
    recursive: bool,
    keep_multi_quality: bool,
    on_progress: &(dyn Fn(usize, usize, &str) + Send + Sync),
    is_paused: &(dyn Fn() -> bool + Send + Sync),
    is_canceled: &(dyn Fn() -> bool + Send + Sync),
) -> Result<Option<CleanupScanReport>> {
    let files = collect_audio_files(dirs, recursive);
    let mut report = CleanupScanReport {
        scanned_files: files.len(),
        ..Default::default()
    };
    if files.is_empty() {
        return Ok(Some(report));
    }
    on_progress(0, files.len(), "");

    // 并发识别（滑动窗口）：联网匹配无旁车文件，串行会太慢。
    let api = Arc::new(api.clone());
    let mut pending: Vec<PathBuf> = files;
    pending.sort();
    let mut set: tokio::task::JoinSet<(PathBuf, Option<u64>, CleanupFileItem, String, String)> =
        tokio::task::JoinSet::new();
    let mut entries: Vec<Entry> = Vec::new();
    const WINDOW: usize = 6;
    let mut completed = 0usize;
    loop {
        if is_canceled() {
            return Ok(None);
        }
        while is_paused() {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if is_canceled() {
                return Ok(None);
            }
        }
        while set.len() < WINDOW {
            let Some(path) = pending.pop() else { break };
            let api = api.clone();
            set.spawn(async move {
                let netease_id = local_file_netease_id(&api, &path)
                    .await
                    .unwrap_or(None);
                let (item, title, artist) = inspect(&path);
                (path, netease_id, item, title, artist)
            });
        }
        let Some(joined) = set.join_next().await else { break };
        let (path, netease_id, item, title, artist) =
            joined.map_err(|e| anyhow::anyhow!("scan task failed: {e}"))?;
        completed += 1;
        on_progress(
            completed,
            report.scanned_files,
            &path.to_string_lossy(),
        );
        report.identified += netease_id.is_some() as usize;
        report.unresolved += netease_id.is_none() as usize;
        entries.push(Entry {
            path,
            netease_id,
            item,
            title,
            artist,
        });
    }

    // 分组：id → 曲目组；无法识别的各自单列（不动）。
    let mut by_song: HashMap<u64, Vec<Entry>> = HashMap::new();
    let mut singles: Vec<Entry> = Vec::new();
    for entry in entries {
        match entry.netease_id {
            Some(id) => by_song.entry(id).or_default().push(entry),
            None => singles.push(entry),
        }
    }

    let mut groups: Vec<CleanupGroup> = Vec::new();
    for (_, members) in by_song {
        if members.len() > 1 {
            report.duplicate_groups += 1;
        }
        groups.push(build_group(members, keep_multi_quality));
    }
    for mut entry in singles {
        entry.item.action = "keep".into();
        let song = display_name(&entry);
        groups.push(CleanupGroup {
            song,
            netease_id: None,
            items: vec![entry.item],
        });
    }
    groups.sort_by(|a, b| a.song.cmp(&b.song));

    for group in &groups {
        for item in &group.items {
            if item.action == "clean" {
                report.clean_count += 1;
                report.clean_bytes += item.size;
            }
        }
    }
    report.groups = groups;
    Ok(Some(report))
}

/// 递归/非递归收集音频文件（排除隔离区目录）。
pub(crate) fn collect_audio_files(dirs: &[String], recursive: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in dirs {
        let dir = Path::new(dir);
        if !dir.is_dir() {
            continue;
        }
        let walker = if recursive {
            WalkDir::new(dir).follow_links(false)
        } else {
            WalkDir::new(dir).max_depth(1).follow_links(false)
        };
        for entry in walker.into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            // 隔离区里的文件不参与扫描。
            if path.components().any(|c| c.as_os_str() == ".quarantine") {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.to_ascii_lowercase());
            if let Some(ext) = ext {
                if AUDIO_EXTS.contains(&ext.as_str()) {
                    files.push(path.to_path_buf());
                }
            }
        }
    }
    files
}

/// 读单个文件的音质、标签属性（零网络）。文件读不出信息时按扩展名兜底。
fn inspect(path: &Path) -> (CleanupFileItem, String, String) {
    let (quality, quality_key, bitrate) = quality_of(path);
    let mut item = CleanupFileItem {
        path: path.to_string_lossy().into_owned(),
        quality,
        quality_key,
        bitrate,
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        modified_at: std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map(|t| chrono::DateTime::<Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default(),
        score: 0,
        has_title: false,
        has_artist: false,
        has_album: false,
        has_cover: false,
        has_lyrics: false,
        has_comment: false,
        has_sidecar: false,
        action: "keep".into(),
    };
    let mut title = String::new();
    let mut artist = String::new();
    if let Ok(tagged) = Probe::open(path).and_then(|p| p.read()) {
        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            item.has_title = !tag.title().map(|v| v.is_empty()).unwrap_or(true);
            item.has_artist = !tag.artist().map(|v| v.is_empty()).unwrap_or(true);
            item.has_album = !tag.album().map(|v| v.is_empty()).unwrap_or(true);
            item.has_cover = !tag.pictures().is_empty();
            item.has_lyrics = tag.get(&ItemKey::Lyrics).is_some();
            item.has_comment = tag.get(&ItemKey::Comment).is_some();
            title = tag.title().map(|v| v.into_owned()).unwrap_or_default();
            artist = tag.artist().map(|v| v.into_owned()).unwrap_or_default();
        }
    }
    item.has_sidecar = crate::core::sync::sidecar_path(path).is_file();
    let flags = [
        item.has_title,
        item.has_artist,
        item.has_album,
        item.has_cover,
        item.has_lyrics,
        item.has_comment,
        item.has_sidecar,
    ];
    item.score = flags.iter().filter(|f| **f).count() as u32;
    (item, title, artist)
}

/// 音质判定（用户约定 2026-09）：只按扩展名区分（mp3/flac/...），
/// 码率单独带回，供"同后缀保留最高音质"比较与展示。
fn quality_of(path: &Path) -> (String, String, u32) {
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .map(|x| x.to_ascii_lowercase())
        .unwrap_or_default();
    let mut bitrate: u32 = 0;
    if let Ok(tagged) = Probe::open(path).and_then(|p| p.read()) {
        let props = tagged.properties();
        bitrate = props.audio_bitrate().or(props.overall_bitrate()).unwrap_or(0);
    }
    (ext.to_uppercase(), ext, bitrate)
}

fn display_name(entry: &Entry) -> String {
    if !entry.artist.is_empty() && !entry.title.is_empty() {
        format!("{} - {}", entry.artist, entry.title)
    } else if !entry.title.is_empty() {
        entry.title.clone()
    } else {
        entry
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned()
    }
}

/// 组内决定保留/清理：按音质键（=扩展名；保留多音质=false 时不分组）分组。
/// 多音质模式：同后缀内码率高者优先（"只保留最高音质"），再比属性、时间；
/// 单文件模式：属性最全优先，其次最新。各留一个。
fn build_group(members: Vec<Entry>, keep_multi_quality: bool) -> CleanupGroup {
    let rank_by_bitrate = |a: &Entry, b: &Entry| {
        b.item
            .bitrate
            .cmp(&a.item.bitrate)
            .then(b.item.score.cmp(&a.item.score))
            .then(b.item.modified_at.cmp(&a.item.modified_at))
            .then(a.item.path.cmp(&b.item.path))
    };
    let rank_by_score = |a: &Entry, b: &Entry| {
        b.item
            .score
            .cmp(&a.item.score)
            .then(b.item.modified_at.cmp(&a.item.modified_at))
            .then(a.item.path.cmp(&b.item.path))
    };
    // 保留项的展示名要用标签，先留存 path → (artist, title)。
    let meta: HashMap<String, (String, String)> = members
        .iter()
        .map(|e| (e.item.path.clone(), (e.artist.clone(), e.title.clone())))
        .collect();
    let netease_id = members.first().and_then(|e| e.netease_id);
    let mut subgroups: Vec<Vec<Entry>> = if keep_multi_quality {
        let mut by_quality: HashMap<String, Vec<Entry>> = HashMap::new();
        for entry in members {
            by_quality
                .entry(entry.item.quality_key.clone())
                .or_default()
                .push(entry);
        }
        let mut keys: Vec<String> = by_quality.keys().cloned().collect();
        keys.sort();
        keys.into_iter().map(|k| by_quality.remove(&k).unwrap()).collect()
    } else {
        vec![members]
    };
    let mut items: Vec<CleanupFileItem> = Vec::new();
    let mut keep_path = String::new();
    for group in &mut subgroups {
        group.sort_by(if keep_multi_quality {
            rank_by_bitrate
        } else {
            rank_by_score
        });
        for (index, entry) in group.iter_mut().enumerate() {
            entry.item.action = if index == 0 { "keep" } else { "clean" }.into();
            if index == 0 && keep_path.is_empty() {
                keep_path = entry.item.path.clone();
            }
            items.push(entry.item.clone());
        }
    }
    // 展示顺序：保留项在前。
    items.sort_by(|a, b| a.action.cmp(&b.action).then(a.path.cmp(&b.path)));
    let song = meta
        .get(&keep_path)
        .map(|(artist, title)| match (artist.is_empty(), title.is_empty()) {
            (false, false) => format!("{artist} - {title}"),
            (true, false) => title.clone(),
            _ => Path::new(&keep_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned(),
        })
        .unwrap_or_default();
    CleanupGroup {
        song,
        netease_id,
        items,
    }
}

/// 执行清理：把指定文件移入**它自己所在文件夹**的 `.quarantine\cleanup\`
/// 并登记（复用隔离区页可恢复）。文件留在原处附近，恢复距离最短，
/// 也不会在音乐根目录或无关文件夹里冒出隔离区。
/// 返回成功隔离的文件数。track_files 中指向被清文件的记录一并删除。
pub fn execute(conn: &mut rusqlite::Connection, paths: &[String], sync_run_id: i64) -> Result<usize> {
    let mut cleaned = 0usize;
    for path in paths {
        let source = PathBuf::from(path);
        if !source.is_file() {
            continue;
        }
        let Some(source_dir) = source.parent() else {
            continue;
        };
        let quarantine_dir = source_dir.join(".quarantine").join(CLEANUP_SOURCE);
        std::fs::create_dir_all(&quarantine_dir)?;
        let file_name = source
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or_default()
            .to_owned();
        // 文件名追加 .quarantined 后缀 + 隐藏属性，防客户端按扩展名收录（见 quarantine_files）。
        let target = unique_quarantine_path(
            &quarantine_dir,
            &crate::core::quarantine_files::quarantined_filename(&file_name),
        );
        move_aside(&source, &target)?;
        crate::core::quarantine_files::set_hidden(&target, true);
        // 伴生文件（尽力而为）。
        let lrc = source.with_extension("lrc");
        if lrc.is_file() {
            let _ = move_aside(&lrc, &target.with_extension("lrc"));
        }
        let sidecar = crate::core::sync::sidecar_path(&source);
        if sidecar.is_file() {
            let sidecar_target = crate::core::sync::sidecar_path(&target);
            let _ = move_aside(&sidecar, &sidecar_target);
        }
        crate::store::database::add_quarantine(
            conn,
            0,
            CLEANUP_SOURCE,
            &file_name,
            &source.to_string_lossy(),
            &target.to_string_lossy(),
        )?;
        let _ = crate::store::database::record_deleted(
            conn,
            "local_file",
            0,
            CLEANUP_SOURCE,
            None,
            None,
            Some(&source.to_string_lossy()),
            Some(&target.to_string_lossy()),
            None,
            Some("duplicate cleanup"),
        )?;
        // 已登记"已同步"的路径失效，下次同步按现状重新登记。
        let _ = conn.execute("DELETE FROM track_files WHERE local_path=?1", [path]);
        // 任务详情：记一条变更供展示。
        let _ = crate::store::database::record_change(
            conn,
            sync_run_id,
            0,
            CLEANUP_SOURCE,
            "to_quarantine",
            "quarantined_local",
            None,
            Some(&file_name),
            Some(&source.to_string_lossy()),
            Some(&target.to_string_lossy()),
            None,
            Some("duplicate cleanup"),
        );
        cleaned += 1;
    }
    Ok(cleaned)
}

/// 移动文件；跨盘 rename 失败时退回复制+删除。
fn move_aside(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(source, target).is_ok() {
        return Ok(());
    }
    std::fs::copy(source, target)?;
    std::fs::remove_file(source)?;
    Ok(())
}

fn unique_quarantine_path(dir: &Path, filename: &str) -> PathBuf {
    let time = Local::now().format("%Y%m%d-%H%M%S");
    let mut candidate = dir.join(format!("{time}_{filename}"));
    let mut counter = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{time}_{counter}_{filename}"));
        counter += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, score: u32, modified_at: &str, bitrate: u32) -> Entry {
        let ext = Path::new(path)
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or("mp3")
            .to_ascii_lowercase();
        Entry {
            path: PathBuf::from(path),
            netease_id: Some(1),
            item: CleanupFileItem {
                path: path.to_string(),
                quality: ext.to_uppercase(),
                quality_key: ext,
                bitrate,
                size: 0,
                modified_at: modified_at.to_string(),
                score,
                has_title: false,
                has_artist: false,
                has_album: false,
                has_cover: false,
                has_lyrics: false,
                has_comment: false,
                has_sidecar: false,
                action: "keep".into(),
            },
            title: String::new(),
            artist: String::new(),
        }
    }

    #[test]
    fn keeps_most_complete_then_newest() {
        let members = vec![
            entry("a.mp3", 3, "2026-01-02 00:00:00", 320_000),
            entry("b.mp3", 5, "2026-01-01 00:00:00", 320_000),
            entry("c.mp3", 5, "2026-01-03 00:00:00", 320_000),
        ];
        let group = build_group(members, false);
        let keep: Vec<&str> = group
            .items
            .iter()
            .filter(|i| i.action == "keep")
            .map(|i| i.path.as_str())
            .collect();
        // 单文件模式：同分取最新，音质不参与。
        assert_eq!(keep, vec!["c.mp3"]);
        assert_eq!(group.items.len(), 3);
    }

    #[test]
    fn keep_multi_quality_keeps_one_per_extension() {
        let members = vec![
            entry("a.mp3", 5, "2026-01-01 00:00:00", 320_000),
            entry("b.mp3", 2, "2026-01-02 00:00:00", 320_000),
            entry("c.flac", 1, "2026-01-02 00:00:00", 900_000),
        ];
        let group = build_group(members, true);
        let keep: Vec<&str> = group
            .items
            .iter()
            .filter(|i| i.action == "keep")
            .map(|i| i.path.as_str())
            .collect();
        // 每种后缀各留一份：mp3 组同码率比属性 → a；flac 留 c。
        assert_eq!(keep, vec!["a.mp3", "c.flac"]);
    }

    #[test]
    fn same_extension_keeps_highest_bitrate() {
        let members = vec![
            entry("a.mp3", 5, "2026-01-02 00:00:00", 128_000),
            entry("b.mp3", 2, "2026-01-01 00:00:00", 320_000),
        ];
        let group = build_group(members, true);
        let keep: Vec<&str> = group
            .items
            .iter()
            .filter(|i| i.action == "keep")
            .map(|i| i.path.as_str())
            .collect();
        // 同后缀只保留最高音质：码率优先于属性完整度。
        assert_eq!(keep, vec!["b.mp3"]);
    }

    #[test]
    fn single_quality_mode_cleans_other_extensions() {
        let members = vec![
            entry("a.mp3", 5, "2026-01-01 00:00:00", 320_000),
            entry("c.flac", 1, "2026-01-02 00:00:00", 900_000),
        ];
        let group = build_group(members, false);
        let keep: Vec<&str> = group
            .items
            .iter()
            .filter(|i| i.action == "keep")
            .map(|i| i.path.as_str())
            .collect();
        assert_eq!(keep, vec!["a.mp3"], "only the most complete file survives");
    }
}
