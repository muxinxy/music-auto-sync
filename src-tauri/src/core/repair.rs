//! 文件属性修复工具：按网易曲目元数据修补本地音频的详细信息、封面、歌词，
//! 并可选把文件名规范为 `{歌手} - {标题}`。
//!
//! 曲目识别复用 `local_file_netease_id`（旁车 → 163 key → 联网 tag 匹配），
//! 无法识别的文件跳过不动。内容修复（标签/封面/歌词）先于重命名执行。

use std::path::{Path, PathBuf};

use anyhow::Result;
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::Accessor;

use crate::api::{NeteaseApi, Track};

/// MP3 帧同步是否合法（同步位 + 版本/层/码率/采样率字段全部非保留值）。
fn mp3_sync_at(bytes: &[u8], i: usize) -> bool {
    bytes[i] == 0xFF
        && (bytes[i + 1] & 0xE0) == 0xE0
        && (bytes[i + 1] & 0x18) != 0x08      // 版本保留值
        && (bytes[i + 1] & 0x06) != 0x00      // 层保留值
        && (bytes[i + 2] & 0xF0) != 0xF0      // 码率 bad
        && (bytes[i + 2] & 0xF0) != 0x00      // 码率 free
        && (bytes[i + 2] & 0x0C) != 0x0C      // 采样率保留值
}

/// 从 `from` 起找第一个像真的 MP3 帧同步：其后一个最大帧长窗口内还要有
/// 下一个同步（防正文里的偶同步误匹配）。
fn find_audio_sync(bytes: &[u8], from: usize) -> Option<usize> {
    let end = (from + 4 * 1024 * 1024).min(bytes.len().saturating_sub(4));
    let mut i = from;
    while i < end {
        if mp3_sync_at(bytes, i) {
            // MPEG2/2.5 Layer III 最大帧 ~2880 字节。
            let next_end = (i + 2882).min(bytes.len() - 4);
            if ((i + 32)..next_end).any(|j| mp3_sync_at(bytes, j)) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 剥离无法解析的 ID3v2 标签，让文件恢复可解析。
///
/// QQMusic 出品的文件在正常帧之后有 1024 字节 padding + 一段 QMCI 私有数据，
/// 标签头声明的大小既不等于帧尾也不落在音频头上，lofty 因此整文件解析失败。
/// 所以候选偏移按序尝试：声明大小（syncsafe/v2.4 与大端/v2.3 两种口径）→
/// 逐帧走到停下的帧尾 → 帧尾之后第一个合法 MP3 同步。每个候选剥离后都用
/// lofty 复检，全部失败则还原原文件内容。
/// 返回是否成功剥离。
fn strip_broken_id3v2(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    if bytes.len() < 20 || &bytes[..3] != b"ID3" {
        return false;
    }
    let version = bytes[3];
    let syncsafe = ((bytes[6] as usize & 0x7f) << 21)
        | ((bytes[7] as usize & 0x7f) << 14)
        | ((bytes[8] as usize & 0x7f) << 7)
        | (bytes[9] as usize & 0x7f);
    let plain = ((bytes[6] as usize) << 24)
        | ((bytes[7] as usize) << 16)
        | ((bytes[8] as usize) << 8)
        | bytes[9] as usize;

    let mut candidates = vec![syncsafe + 10, plain + 10];
    // 逐帧走（v2.4 帧长是 syncsafe、v2.3 是大端），停在第一个非法帧头。
    let mut off = 10usize;
    while off + 10 <= bytes.len() {
        let id = &bytes[off..off + 4];
        if !id.iter().all(u8::is_ascii_alphanumeric) {
            break;
        }
        let size = if version >= 4 {
            ((bytes[off + 4] as usize & 0x7f) << 21)
                | ((bytes[off + 5] as usize & 0x7f) << 14)
                | ((bytes[off + 6] as usize & 0x7f) << 7)
                | (bytes[off + 7] as usize & 0x7f)
        } else {
            ((bytes[off + 4] as usize) << 24)
                | ((bytes[off + 5] as usize) << 16)
                | ((bytes[off + 6] as usize) << 8)
                | bytes[off + 7] as usize
        };
        if size == 0 || off + 10 + size > bytes.len() {
            break;
        }
        off += 10 + size;
    }
    if off > 10 {
        candidates.push(off);
        if let Some(sync) = find_audio_sync(&bytes, off) {
            candidates.push(sync);
        }
    }

    let looks_like_audio = |b: &[u8]| {
        b.len() >= 10
            && (b[0] == 0xFF && (b[1] & 0xE0) == 0xE0
                || &b[..3] == b"fLaC"
                || &b[..4] == b"OggS"
                || &b[4..8] == *b"ftyp")
    };
    let original = bytes.clone();
    for &size in candidates.iter() {
        if size < 20 || size + 8 > bytes.len() || !looks_like_audio(&bytes[size..]) {
            continue;
        }
        if std::fs::write(path, &bytes[size..]).is_ok() && tag_readable(path) {
            return true;
        }
    }
    // 所有候选都不行：还原原文件，调用方按 tagUnreadable 处理。
    let _ = std::fs::write(path, original);
    false
}

/// 文件标签是否可被 lofty 解析。
fn tag_readable(path: &Path) -> bool {
    Probe::open(path).and_then(|p| p.read()).is_ok()
}

#[derive(Debug, Clone, Default)]
pub struct RepairOptions {
    pub fix_tags: bool,
    pub fix_cover: bool,
    pub fix_lyrics: bool,
    pub fix_filename: bool,
    /// 文件名规范模板（默认与设置里的文件名模板一致）。
    pub filename_template: String,
}

/// 修复任务进度快照（AppState 持有，前端轮询/事件读取）。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairProgress {
    pub current: usize,
    pub total: usize,
    pub repaired: usize,
    pub skipped: usize,
    pub failed: usize,
    pub current_file: String,
    pub done: bool,
    pub canceled: bool,
}

/// 单个文件的修复结果。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairItemResult {
    pub source: String,
    /// 重命名后的新路径（未重命名或失败时与 source 相同 / None）。
    pub output: Option<String>,
    /// repaired | skipped | failed
    pub status: String,
    /// skipped 且无 error = 无法识别曲目；failed / skipped 携带的 error 为具体原因。
    pub error: Option<String>,
}

/// 修复单个文件。`db_paths` 用于重命名后同步 track_files 登记路径。
pub async fn repair_file(
    api: &NeteaseApi,
    path: &Path,
    opts: &RepairOptions,
    artist_separator: &str,
) -> RepairItemResult {
    let source = path.to_string_lossy().into_owned();
    if !opts.fix_tags && !opts.fix_cover && !opts.fix_lyrics && !opts.fix_filename {
        return RepairItemResult {
            source,
            output: None,
            status: "skipped".into(),
            error: Some("nothingToFix".into()),
        };
    }
    // 标签损坏（如历史版本写入无效编码帧）→ 先剥掉 ID3v2 标签，让文件恢复可解析；
    // 识别交给后面的联网匹配（md5 + 时长，不依赖标签文本）。
    if !tag_readable(path) && !strip_broken_id3v2(path) {
        return RepairItemResult {
            source,
            output: None,
            status: "failed".into(),
            error: Some("tagUnreadable".into()),
        };
    }
    let Some(id) = crate::core::sync::local_file_netease_id(api, path)
        .await
        .unwrap_or(None)
    else {
        return RepairItemResult {
            source,
            output: None,
            status: "skipped".into(),
            error: None,
        };
    };
    // 识别成功后补写旁车（163 key 命中的文件可能没有），后续识别零网络。
    let _ = crate::core::sync::write_sidecar(path, 0, id);

    let details = match api.song_detail_batch(&[id]).await {
        Ok(details) => details,
        Err(error) => {
            return RepairItemResult {
                source,
                output: None,
                status: "failed".into(),
                error: Some(error.to_string()),
            };
        }
    };
    let Some(song) = details.get(&id) else {
        return RepairItemResult {
            source,
            output: None,
            status: "failed".into(),
            error: Some("songDetailMissing".into()),
        };
    };
    let track: Track = match serde_json::from_value(song.clone()) {
        Ok(track) => track,
        Err(error) => {
            return RepairItemResult {
                source,
                output: None,
                status: "failed".into(),
                error: Some(format!("parseTrack: {error}")),
            };
        }
    };

    // 逐项判断是否真的需要写：属性已完整的部分跳过，避免无谓重写覆盖用户数据。
    let existing = if opts.fix_tags || opts.fix_cover || opts.fix_lyrics {
        Probe::open(path).and_then(|p| p.read()).ok()
    } else {
        None
    };
    let existing_tag = existing.as_ref().and_then(|f| f.primary_tag());
    let (cur_title, cur_artist, cur_album) = existing_tag
        .map(|tag| {
            (
                tag.title().map(|v| v.to_string()).unwrap_or_default(),
                tag.artist().map(|v| v.to_string()).unwrap_or_default(),
                tag.album().map(|v| v.to_string()).unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    let expected_artist = crate::core::naming::artists_with(&track, artist_separator);
    let tags_needed = opts.fix_tags
        && (cur_title != track.name
            || cur_artist != expected_artist
            || cur_album != track.al.name);
    let cover_needed = opts.fix_cover
        && existing_tag
            .map(|tag| tag.pictures().is_empty())
            .unwrap_or(true);
    let lyrics_needed = opts.fix_lyrics
        && existing_tag
            .map(|tag| tag.get(&lofty::tag::ItemKey::Lyrics).is_none())
            .unwrap_or(true);
    let target = if opts.fix_filename {
        target_name(path, &track, &opts.filename_template, artist_separator)
    } else {
        None
    };
    let rename_needed = matches!(&target, Some(t) if t != path && !t.exists());
    let rename_blocked = matches!(&target, Some(t) if t != path && t.exists());

    if !tags_needed && !cover_needed && !lyrics_needed && !rename_needed {
        return RepairItemResult {
            source,
            output: Some(path.to_string_lossy().into_owned()),
            status: "skipped".into(),
            error: Some("alreadyComplete".into()),
        };
    }

    // 内容修复先于重命名：都作用在当前路径上。
    if tags_needed {
        if let Err(error) =
            crate::tags::tags::write_basic_tags(path, &track, 0, artist_separator)
        {
            return failed(source, error);
        }
    }
    if cover_needed {
        if let Some(pic_url) = track.al.pic_url.as_deref() {
            if let Err(error) = crate::tags::tags::write_album_cover(path, pic_url).await {
                return failed(source, error);
            }
        }
    }
    if lyrics_needed {
        match api.lyric(id).await {
            Ok(Some(lyrics)) => {
                if let Err(error) = crate::tags::tags::write_embedded_lyrics(path, &lyrics) {
                    return failed(source, error);
                }
            }
            Ok(None) => {}
            Err(error) => return failed(source, error),
        }
    }

    // 文件名规范为模板.原扩展名。目标已存在时跳过重命名（修复工具不 fork 副本）。
    let mut output = source.clone();
    let mut rename_note: Option<String> = None;
    if rename_needed {
        let target = target.unwrap();
        if let Err(error) = rename_with_companions(path, &target) {
            return failed(source, error);
        }
        output = target.to_string_lossy().into_owned();
    } else if rename_blocked {
        rename_note = Some("targetNameExists".into());
    }
    RepairItemResult {
        source,
        output: Some(output),
        status: "repaired".into(),
        error: rename_note,
    }
}

fn failed(source: String, error: impl std::fmt::Display) -> RepairItemResult {
    RepairItemResult {
        source,
        output: None,
        status: "failed".into(),
        error: Some(error.to_string()),
    }
}

/// 按网易云元数据算出规范文件名（模板.扩展名）。
fn target_name(path: &Path, track: &Track, template: &str, artist_separator: &str) -> Option<PathBuf> {
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .map(|x| x.to_ascii_lowercase())?;
    let name = crate::core::naming::apply_template(template, "", track, 0, artist_separator);
    Some(path.with_file_name(format!("{name}.{ext}")))
}

/// 重命名文件并尽力迁移 .lrc 与旁车。
fn rename_with_companions(source: &Path, target: &Path) -> Result<()> {
    std::fs::rename(source, target)?;
    let lrc = source.with_extension("lrc");
    if lrc.is_file() {
        let _ = std::fs::rename(lrc, target.with_extension("lrc"));
    }
    let sidecar = crate::core::sync::sidecar_path(source);
    if sidecar.is_file() {
        let _ = std::fs::rename(&sidecar, crate::core::sync::sidecar_path(target));
    }
    Ok(())
}

/// 展开用户选择的混合路径（文件 + 目录）为音频文件列表（目录递归）。
pub fn expand_inputs(paths: &[String], recursive: bool) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    for p in paths {
        let path = Path::new(p);
        if path.is_dir() {
            files.extend(crate::core::cleanup::collect_audio_files(
                std::slice::from_ref(p),
                recursive,
            ));
        } else if path.is_file() {
            let is_audio = path
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| {
                    crate::core::cleanup::AUDIO_EXTS.contains(&x.to_ascii_lowercase().as_str())
                })
                .unwrap_or(false);
            if is_audio {
                files.push(path.to_path_buf());
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手动回归：对真实损坏文件（QQMusic QMCI 布局等）的临时副本跑剥离，
    /// 不动原文件。用法：
    /// STRIP_PROBE_PATH="<文件路径>" cargo test strip_real_broken_file -- --ignored --nocapture
    #[test]
    #[ignore = "需要 STRIP_PROBE_PATH 指向真实文件，默认跳过"]
    fn strip_real_broken_file() {
        let source = std::path::PathBuf::from(
            std::env::var("STRIP_PROBE_PATH").expect("set STRIP_PROBE_PATH"),
        );
        let tmp = tempfile::tempdir().expect("tempdir");
        let dest = tmp.path().join("probe.mp3");
        std::fs::copy(&source, &dest).expect("copy");
        assert!(
            !tag_readable(&dest),
            "原始文件应当无法被 lofty 解析（否则选错样本）"
        );
        assert!(strip_broken_id3v2(&dest), "剥离应当成功");
        assert!(tag_readable(&dest), "剥离后应当可解析");
        println!("OK: {}", dest.display());
    }
}
