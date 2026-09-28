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

/// 剥离损坏的 ID3v2 标签（lofty 无法解析时）：按 syncsafe 与普通大端两种口径
/// 计算标签长度，选使后续字节像音频头的那个；剥离后文件可重新解析与写标签。
/// 返回是否成功剥离。
fn strip_broken_id3v2(path: &Path) -> bool {
    let Ok(mut bytes) = std::fs::read(path) else {
        return false;
    };
    if bytes.len() < 20 || &bytes[..3] != b"ID3" {
        return false;
    }
    let syncsafe = ((bytes[6] as usize & 0x7f) << 21)
        | ((bytes[7] as usize & 0x7f) << 14)
        | ((bytes[8] as usize & 0x7f) << 7)
        | (bytes[9] as usize & 0x7f);
    let plain = ((bytes[6] as usize) << 24)
        | ((bytes[7] as usize) << 16)
        | ((bytes[8] as usize) << 8)
        | bytes[9] as usize;
    let looks_like_audio = |b: &[u8]| {
        b.len() >= 10
            && (b[0] == 0xFF && (b[1] & 0xE0) == 0xE0
                || &b[..3] == b"fLaC"
                || &b[..4] == b"OggS"
                || &b[4..8] == *b"ftyp")
    };
    let size = [syncsafe + 10, plain + 10].into_iter().find(|&s| {
        s > 10 && s + 8 < bytes.len() && looks_like_audio(&bytes[s..])
    });
    let Some(size) = size else {
        return false;
    };
    bytes.drain(..size);
    std::fs::write(path, &bytes).is_ok()
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
