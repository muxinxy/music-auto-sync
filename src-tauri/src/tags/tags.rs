use anyhow::{Context, Result};
use lofty::{
    config::WriteOptions,
    file::TaggedFileExt,
    picture::{Picture, PictureType},
    probe::Probe,
    tag::{Accessor, ItemKey, TagExt, TagType},
};
use std::path::Path;

use crate::{api::Track, core::naming::artists_with};

/// 写选项：新写的 ID3v2 一律用 **v2.3**（与网易官方下载一致）。Windows 资源管理器
/// 对 ID3v2.4 的 APIC 支持差，会显示"无封面"；v2.3 兼容性最好。非 ID3v2 标签
/// （Vorbis/MP4）会忽略该选项，不受影响。
fn v23_write_options() -> WriteOptions {
    WriteOptions::new().use_id3v23(true)
}

/// 通用 `Tag` 与 `Id3v2Tag` 互转会丢掉帧级编码信息：图片帧被写成 UTF-16（enc=1）、
/// 163 key 备注被写成 UTF-8（enc=3）——Windows 资源管理器对二者都不解析，
/// 表现为"无封面 / 备注为空"。保存前统一重建为官方格式（enc=0 + lang XXX + 空描述）。
fn reassert_official_frame_encodings(id3: &mut lofty::id3::v2::Id3v2Tag) {
    use lofty::id3::v2::Frame;
    use lofty::tag::items::UNKNOWN_LANGUAGE;
    use lofty::TextEncoding;

    let frames: Vec<Frame<'static>> = std::mem::take(id3)
        .into_iter()
        .map(|frame| match frame {
            Frame::Picture(mut pic) => {
                pic.encoding = TextEncoding::Latin1;
                Frame::Picture(pic)
            }
            Frame::Comment(mut comment) => {
                // 本应用写入的备注只有官方 163 key，按官方格式恢复。
                comment.encoding = TextEncoding::Latin1;
                comment.language = UNKNOWN_LANGUAGE;
                comment.description = String::new();
                Frame::Comment(comment)
            }
            other => other,
        })
        .collect();
    for frame in frames {
        id3.insert(frame);
    }
}

pub fn write_basic_tags(
    path: &Path,
    track: &Track,
    position: usize,
    artist_separator: &str,
) -> Result<()> {
    let artist = artists_with(track, artist_separator);
    let tagged_file = Probe::open(path)?.read()?;
    let tag_type = tagged_file.primary_tag_type();
    if let Some(generic) = tagged_file.primary_tag().cloned() {
        // 走 Id3v2Tag 专用路径：通用 Tag 保存会丢帧级编码（见 reassert_official_frame_encodings）。
        let mut id3: lofty::id3::v2::Id3v2Tag = generic.into();
        id3.set_title(track.name.clone());
        id3.set_artist(artist.clone());
        id3.set_album(track.al.name.clone());
        // position=0 表示无曲目序号（NCM 转换产物），不写 TRCK。
        if position > 0 {
            id3.set_track(position as u32);
        }
        // 不再写纯文本 netease-id；网易 id 以官方 163 key 形式写入（见 write_netease_key）。
        reassert_official_frame_encodings(&mut id3);
        id3.save_to_path(path, v23_write_options())?;
    } else {
        let mut tag = lofty::tag::Tag::new(if tag_type == TagType::Id3v2 {
            tag_type
        } else {
            TagType::Id3v2
        });
        tag.set_title(track.name.clone());
        tag.set_artist(artist);
        tag.set_album(track.al.name.clone());
        if position > 0 {
            tag.set_track(position as u32);
        }
        tag.save_to_path(path, v23_write_options())?;
    }
    Ok(())
}

/// 把专辑封面写入文件主标签（ID3 APIC / Vorbis METADATA_BLOCK_PICTURE 等）。
/// 先异步下载封面字节（15s 超时、UA），再同步嵌入——调用方为 async 下载路径。
/// 失败返回 Err，由调用方决定是否仅告警。
pub async fn write_album_cover(path: &Path, pic_url: &str) -> Result<()> {
    // 大图（原图）可能数 MB，请求带尺寸参数的精简图即可满足播放器/资源管理器封面。
    let url = if pic_url.contains('?') {
        pic_url.to_owned()
    } else {
        format!("{pic_url}?param=500y500")
    };
    let bytes = reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()?
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    embed_cover_bytes(path, &bytes)
}

/// 已持有封面字节时同步嵌入（供重复嵌入避免二次下载）。
/// ID3v2 主标签用**原生 AttachedPictureFrame 且描述 Latin1 编码（enc=0）**——Windows
/// 资源管理器对 UTF-16 编码的 APIC 支持差（会显示"无封面"），官方网易文件也是 enc=0。
/// 非 ID3v2（flac/ogg/m4a 等）走通用 Tag（对应格式无此兼容问题）。
pub(crate) fn embed_cover_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() < 1024 {
        anyhow::bail!("album art too small ({} bytes)", bytes.len());
    }
    let mut picture = Picture::from_reader(&mut &bytes[..])?;
    picture.set_pic_type(PictureType::CoverFront);
    let mut tagged_file = Probe::open(path)
        .with_context(|| format!("cannot open tag of {}", path.display()))?
        .read()?;

    if tagged_file.primary_tag_type() == TagType::Id3v2 {
        use lofty::id3::v2::{AttachedPictureFrame, Frame, Id3v2Tag};
        use lofty::TextEncoding;
        if let Some(generic) = tagged_file.primary_tag().cloned() {
            let mut id3: Id3v2Tag = generic.into();
            // 清掉全部旧图片帧（不同 PictureType 的旧封面会残留），再写入新封面。
            let kept: Vec<Frame<'static>> = std::mem::take(&mut id3)
                .into_iter()
                .filter(|frame| !matches!(frame, Frame::Picture(_)))
                .collect();
            for frame in kept {
                id3.insert(frame);
            }
            let frame = AttachedPictureFrame::new(TextEncoding::Latin1, picture);
            id3.insert(Frame::Picture(frame));
            id3.save_to_path(path, v23_write_options())?;
            return Ok(());
        }
    }

    // 非 ID3v2（flac/ogg/m4a）：通用 Tag 写入（无编码兼容问题）。
    let Some(tag) = tagged_file.primary_tag_mut() else {
        anyhow::bail!("file has no primary tag");
    };
    tag.set_picture(0, picture);
    tag.save_to_path(path, v23_write_options())?;
    Ok(())
}

/// 把歌词嵌入文件主标签（ID3v2 → USLT 帧、Vorbis/FLAC → LYRICS 字段，
/// lofty 按主标签类型自动映射）。`lrc_text` 为网易 .lrc 原文（可含时间戳行，播放器整段显示）。
pub fn write_embedded_lyrics(path: &Path, lrc_text: &str) -> Result<()> {
    let mut tagged_file = Probe::open(path)
        .with_context(|| format!("cannot open tag of {}", path.display()))?
        .read()?;

    // ID3v2 专用路径：通用 Tag 保存会丢帧级编码，把既有封面/163 key 写坏（见 reassert）。
    if tagged_file.primary_tag_type() == TagType::Id3v2 {
        use lofty::id3::v2::{Frame, Id3v2Tag, UnsynchronizedTextFrame};
        use lofty::tag::items::UNKNOWN_LANGUAGE;
        use lofty::TextEncoding;
        if let Some(generic) = tagged_file.primary_tag().cloned() {
            let mut id3: Id3v2Tag = generic.into();
            // 通用转换已把旧 Lyrics 变成 USLT 帧，全部移除防叠加，再写新歌词。
            let kept: Vec<Frame<'static>> = std::mem::take(&mut id3)
                .into_iter()
                .filter(|frame| !matches!(frame, Frame::UnsynchronizedText(_)))
                .collect();
            for frame in kept {
                id3.insert(frame);
            }
            let frame = UnsynchronizedTextFrame::new(
                TextEncoding::UTF16,
                UNKNOWN_LANGUAGE,
                String::new(),
                lrc_text.to_owned(),
            );
            id3.insert(Frame::UnsynchronizedText(frame));
            reassert_official_frame_encodings(&mut id3);
            id3.save_to_path(path, v23_write_options())?;
            return Ok(());
        }
    }

    let Some(tag) = tagged_file.primary_tag_mut() else {
        anyhow::bail!("file has no primary tag");
    };
    // 覆盖旧值，避免重复嵌入累积。
    tag.remove_key(&ItemKey::Lyrics);
    tag.insert_text(ItemKey::Lyrics, lrc_text.to_owned());
    tag.save_to_path(path, v23_write_options())?;
    Ok(())
}

/// 把官方格式的 163 key 写入 ID3v2 备注（COMM）帧，**字节级复刻网易官方**：
/// enc=0(Latin1) + lang="XXX" + description 空 + `163 key(Don't modify):<b64>`。
/// 用拉丁 1 编码是因为 Windows 资源管理器不解析 UTF-8 编码的 COMM（会显示备注为空）。
///
/// 仅对 MP3 等 ID3v2 主标签生效；非 ID3v2（flac 等）回退到通用 Tag 写入（Vorbis 注释无编码问题）。
pub fn write_netease_key(path: &Path, key_text: &str) -> Result<()> {
    use lofty::id3::v2::{CommentFrame, Frame, Id3v2Tag};
    use lofty::tag::items::UNKNOWN_LANGUAGE;
    use lofty::TextEncoding;
    use crate::core::netease_key::KEY_PREFIX;

    let mut tagged_file = Probe::open(path)
        .with_context(|| format!("cannot open tag of {}", path.display()))?
        .read()?;

    // 仅当文件主标签确实是 ID3v2 时走专用帧（Latin1）路径。
    if tagged_file.primary_tag_type() == TagType::Id3v2 {
        if let Some(generic) = tagged_file.primary_tag().cloned() {
            let mut id3: Id3v2Tag = generic.into();
            // 移除旧的 163 key / 旧 netease-id 备注，避免叠加。
            id3.remove_comment(); // 空 description 的 COMM
            let frame = CommentFrame::new(
                TextEncoding::Latin1,
                UNKNOWN_LANGUAGE,
                String::new(),
                key_text.to_owned(),
            );
            id3.insert(Frame::Comment(frame));
            // 转换过程可能把既有封面/备注写成 UTF-16/UTF-8，统一恢复官方格式。
            reassert_official_frame_encodings(&mut id3);
            id3.save_to_path(path, v23_write_options())?;
            return Ok(());
        }
    }

    // 回退（flac/ogg 等非 ID3v2，或读不到主标签）：用通用 Tag 写入（Vorbis 注释无编码问题）。
    if let Some(tag) = tagged_file.primary_tag_mut() {
        if let Some(item) = tag.get(&ItemKey::Comment) {
            if let Some(text) = item.value().text() {
                if text.contains(KEY_PREFIX) || text.contains("netease-id:") {
                    tag.remove_key(&ItemKey::Comment);
                }
            }
        }
        tag.insert_text(ItemKey::Comment, key_text.to_owned());
        tag.save_to_path(path, v23_write_options())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn writes_latin1_comment_frame_like_official() {
        // 用真实 mp3 副本验证（文件存在才跑，避免 CI 无此路径失败）。
        let src = Path::new(r"D:\Drive\Music\网易云歌单\书影视音乐\毛不易 - 不染.mp3");
        if !src.exists() {
            eprintln!("skip: source mp3 not present");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("test.mp3");
        fs::copy(src, &copy).unwrap();
        let key = crate::core::netease_key::encrypt_official(
            &crate::core::netease_key::OfficialKeyMeta {
                music_id: 1303019637,
                music_name: "说书人".into(),
                artists: vec![("暗杠".into(), 0)],
                album: String::new(),
                album_id: 0,
                album_pic_doc_id: None,
                album_pic: None,
                bitrate: None,
                mp3_doc_id: None,
                duration: 0,
                mv_id: 0,
                format: "mp3".into(),
            },
        );
        write_netease_key(&copy, &key).unwrap();
        // 读回 COMM 帧，验证 enc=0 且内容含 163 key。
        let mb = fs::read(&copy).unwrap();
        assert_eq!(&mb[..3], b"ID3");
        let sz = ((mb[6] as usize & 0x7f) << 21)
            | ((mb[7] as usize & 0x7f) << 14)
            | ((mb[8] as usize & 0x7f) << 7)
            | (mb[9] as usize & 0x7f);
        let mut off = 10usize;
        let mut found = false;
        while off + 10 <= 10 + sz {
            let id = std::str::from_utf8(&mb[off..off + 4]).unwrap_or("");
            let fsize = ((mb[off + 4] as usize) << 24)
                | ((mb[off + 5] as usize) << 16)
                | ((mb[off + 6] as usize) << 8)
                | mb[off + 7] as usize;
            if id == "COMM" {
                let raw = &mb[off + 10..off + 10 + fsize];
                assert_eq!(raw[0], 0, "COMM encoding should be Latin1(0)");
                assert_eq!(&raw[1..4], b"XXX");
                assert_eq!(raw[4], 0, "description should be empty");
                let text = std::str::from_utf8(&raw[5..]).unwrap();
                assert!(
                    text.starts_with("163 key(Don't modify):"),
                    "unexpected COMM text"
                );
                found = true;
            }
            off += 10 + fsize;
        }
        assert!(found, "COMM frame not found");
    }

    #[tokio::test]
    async fn embeds_real_album_cover_from_network() {
        // 用真实 mp3 副本 + 网络拉一张真实封面验证（任一缺失即跳过）。
        let src = Path::new(r"D:\Drive\Music\网易云歌单\书影视音乐\毛不易 - 不染.mp3");
        if !src.exists() {
            eprintln!("skip: source mp3 not present");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("test.mp3");
        fs::copy(src, &copy).unwrap();

        // 先写基础标签（保证有主标签可挂 APIC）。
        let track = crate::api::Track {
            id: 1303019637,
            name: "说书人".into(),
            ar: vec![crate::api::Artist {
                name: "暗杠".into(),
            }],
            al: crate::api::Album {
                id: 0,
                name: String::new(),
                pic_url: None,
            },
            dt: 0,
            no: 1,
        };
        write_basic_tags(&copy, &track, 1, "、").unwrap();

        // 真实封面 URL：网易 1303019637 曲目（若网络不可用则跳过）。
        let url = "https://p1.music.126.net/8G2rC6qLtGQ8kVNq6dX0qg==/109951163128957853.jpg";
        let bytes = match reqwest::Client::builder()
            .user_agent("Mozilla/5.0")
            .timeout(std::time::Duration::from_secs(10))
            .build()
        {
            Ok(client) => match client.get(url).send().await {
                Ok(resp) => match resp.error_for_status() {
                    Ok(resp) => match resp.bytes().await {
                        Ok(bytes) => bytes.to_vec(),
                        Err(_) => {
                            eprintln!("skip: could not read album art (network?)");
                            return;
                        }
                    },
                    Err(_) => {
                        eprintln!("skip: album art fetch failed (network?)");
                        return;
                    }
                },
                Err(_) => {
                    eprintln!("skip: album art fetch failed (network?)");
                    return;
                }
            },
            Err(_) => {
                eprintln!("skip: could not build client");
                return;
            }
        };
        embed_cover_bytes(&copy, &bytes).unwrap();

        // 读回：主标签应含 1 张 CoverFront 图片。
        let tagged = Probe::open(&copy).unwrap().read().unwrap();
        let tag = tagged.primary_tag().unwrap();
        let pictures = tag.pictures();
        assert_eq!(pictures.len(), 1, "expected one embedded picture");
        assert_eq!(
            pictures[0].pic_type(),
            lofty::picture::PictureType::CoverFront
        );
    }

    #[test]
    fn embeds_and_reads_back_lyrics_on_real_mp3() {
        // 用真实 mp3 副本验证歌词可嵌入并被读回（文件缺失即跳过）。
        let src = Path::new(r"D:\Drive\Music\网易云歌单\书影视音乐\毛不易 - 不染.mp3");
        if !src.exists() {
            eprintln!("skip: source mp3 not present");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("test.mp3");
        fs::copy(src, &copy).unwrap();

        let lrc = "[00:01.00]第一行歌词\n[00:05.00]第二行歌词\n";
        write_embedded_lyrics(&copy, lrc).unwrap();

        // 读回：lofty 通用 Tag 里按 Lyrics 键应能取到我们写入的文本。
        let tagged = Probe::open(&copy).unwrap().read().unwrap();
        let tag = tagged.primary_tag().unwrap();
        let stored = tag
            .get(&lofty::tag::ItemKey::Lyrics)
            .map(|item| item.value().text().unwrap_or_default())
            .unwrap_or_default();
        assert!(stored.contains("第一行歌词"), "lyrics not read back");
    }

    /// 回归：先写封面（Latin1 APIC + 163 key 备注），再嵌歌词——通用 Tag 保存
    /// 会把帧级编码重置为 UTF-16/UTF-8，资源管理器随即"无封面/无备注"。
    /// 所有 ID3v2 写路径保存前必须恢复官方编码。
    #[test]
    fn lyrics_write_keeps_apic_and_comment_latin1() {
        // 用真实 mp3 副本验证（文件缺失即跳过）。
        let src = Path::new(r"D:\Drive\Music\网易云歌单\书影视音乐\毛不易 - 不染.mp3");
        if !src.exists() {
            eprintln!("skip: source mp3 not present");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("t.mp3");
        fs::copy(src, &copy).unwrap();


        // 内置 1x1 合法 JPEG 作为嵌入素材（避免依赖源文件封面）。
        use base64::Engine as _;
        let mut cover = base64::engine::general_purpose::STANDARD            .decode(
                "/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwcJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAEDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD3+iiigD//2Q==",
            )
            .unwrap();
        // 尾部补零越过 1024 字节最小体积校验（JPEG 解码忽略 EOI 后的数据）。
        cover.extend(std::iter::repeat(0u8).take(2048 - cover.len()));
        // 与真实下载流程同序：基础标签 → 封面 → 163 key → 歌词。
        write_basic_tags(
            &copy,
            &crate::api::Track {
                id: 1303019637,
                name: "说书人".into(),
                ar: vec![crate::api::Artist { name: "暗杠".into() }],
                al: Default::default(),
                dt: 0,
                no: 1,
            },
            1,
            "、",
        )
        .unwrap();
        embed_cover_bytes(&copy, &cover).unwrap();

        // 封面先落盘（enc=0），随后写 163 key 与歌词；歌词写入不得破坏既有编码。
        write_netease_key(&copy, "163 key(Don't modify):dummy").unwrap();
        write_embedded_lyrics(&copy, "[00:01.00]行\n").unwrap();

        // 逐帧解析：APIC 与 COMM 的 enc 都必须仍是 0。
        let fd = fs::read(&copy).unwrap();
        let sz = ((fd[6] as usize) << 21) | ((fd[7] as usize) << 14) | ((fd[8] as usize) << 7) | fd[9] as usize;
        let mut off = 10usize;
        let mut apic_enc: Option<u8> = None;
        let mut comm_enc: Option<u8> = None;
        while off + 10 <= 10 + sz {
            let id = std::str::from_utf8(&fd[off..off + 4]).unwrap_or("");
            let fsize = ((fd[off + 4] as usize) << 24)
                | ((fd[off + 5] as usize) << 16)
                | ((fd[off + 6] as usize) << 8)
                | fd[off + 7] as usize;
            if id == "APIC" {
                apic_enc = Some(fd[off + 10]);
            }
            if id == "COMM" {
                comm_enc = Some(fd[off + 10]);
            }
            off += 10 + fsize;
        }
        assert_eq!(apic_enc, Some(0), "APIC must stay Latin1 after lyrics write");
        assert_eq!(comm_enc, Some(0), "COMM must stay Latin1 after lyrics write");
    }

    #[test]
    fn writes_id3v23_header_for_windows_compatibility() {
        // 关键兼容性约束：Windows 资源管理器/多数播放器不认 ID3v2.4 的 APIC，
        // 新写标签必须落成 v2.3（与网易官方下载一致），否则封面"看不见"。
        let src = Path::new(r"D:\Drive\Music\网易云歌单\书影视音乐\毛不易 - 不染.mp3");
        if !src.exists() {
            eprintln!("skip: source mp3 not present");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("test.mp3");
        fs::copy(src, &copy).unwrap();
        let track = crate::api::Track {
            id: 1303019637,
            name: "说书人".into(),
            ar: vec![crate::api::Artist {
                name: "暗杠".into(),
            }],
            al: crate::api::Album {
                id: 0,
                name: String::new(),
                pic_url: None,
            },
            dt: 0,
            no: 1,
        };
        write_basic_tags(&copy, &track, 1, "、").unwrap();
        write_embedded_lyrics(&copy, "[00:01.00]行\n").unwrap();

        let fd = fs::read(&copy).unwrap();
        assert_eq!(&fd[..3], b"ID3");
        assert_eq!(fd[3], 3, "ID3 major version must be 3 (v2.3)");
    }

    #[test]
    fn writes_cover_and_track_frames_are_preserved_through_sequence() {
        // 下载后的完整写标签序列（歌词→基础标签→163key→封面）不得丢失 TRCK/APIC。
        let src = Path::new(r"D:\Drive\Music\网易云歌单\书影视音乐\毛不易 - 不染.mp3");
        if !src.exists() {
            eprintln!("skip: source mp3 not present");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("t.mp3");
        fs::copy(src, &copy).unwrap();
        let track = crate::api::Track {
            id: 1303019637,
            name: "说书人".into(),
            ar: vec![crate::api::Artist {
                name: "暗杠".into(),
            }],
            al: crate::api::Album {
                id: 0,
                name: String::new(),
                pic_url: None,
            },
            dt: 0,
            no: 1,
        };
        write_basic_tags(&copy, &track, 1, "、").unwrap();
        write_embedded_lyrics(&copy, "[00:01.00]行\n").unwrap();
        write_netease_key(&copy, "163 key(Don't modify):dummy").unwrap();
        write_basic_tags(&copy, &track, 1, "、").unwrap();

        // 用 lofty 读回：TRCK 与已写帧必须完整保留。
        let tagged = lofty::probe::Probe::open(&copy).unwrap().read().unwrap();
        let tag = tagged.primary_tag().unwrap();
        let track_no = tag
            .get(&lofty::tag::ItemKey::TrackNumber)
            .map(|item| item.value().text().unwrap_or_default().to_owned())
            .unwrap_or_default();
        assert_eq!(track_no, "1", "TRCK must survive the tag sequence");
    }
}
