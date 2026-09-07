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

pub fn write_basic_tags(
    path: &Path,
    track: &Track,
    position: usize,
    artist_separator: &str,
) -> Result<()> {
    let artist = artists_with(track, artist_separator);
    let mut tagged_file = Probe::open(path)?.read()?;
    let tag_type = tagged_file.primary_tag_type();
    if let Some(tag) = tagged_file.primary_tag_mut() {
        tag.set_title(track.name.clone());
        tag.set_artist(artist.clone());
        tag.set_album(track.al.name.clone());
        tag.set_track(position as u32);
        // 不再写纯文本 netease-id；网易 id 以官方 163 key 形式写入（见 write_netease_key）。
        tag.save_to_path(path, WriteOptions::default())?;
    } else {
        let mut tag = lofty::tag::Tag::new(if tag_type == TagType::Id3v2 {
            tag_type
        } else {
            TagType::Id3v2
        });
        tag.set_title(track.name.clone());
        tag.set_artist(artist);
        tag.set_album(track.al.name.clone());
        tag.set_track(position as u32);
        tag.save_to_path(path, WriteOptions::default())?;
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
fn embed_cover_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() < 1024 {
        anyhow::bail!("album art too small ({} bytes)", bytes.len());
    }
    let mut picture = Picture::from_reader(&mut &bytes[..])?;
    picture.set_pic_type(PictureType::CoverFront);
    let mut tagged_file = Probe::open(path)
        .with_context(|| format!("cannot open tag of {}", path.display()))?
        .read()?;
    let Some(tag) = tagged_file.primary_tag_mut() else {
        anyhow::bail!("file has no primary tag");
    };
    tag.set_picture(0, picture);
    tag.save_to_path(path, WriteOptions::default())?;
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
            id3.save_to_path(path, WriteOptions::default())?;
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
        tag.save_to_path(path, WriteOptions::default())?;
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
        let src = Path::new(r"D:\Drive\Music\网易云歌单\古风戏腔\暗杠、寅子 - 说书人.mp3");
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
            let fsize = ((mb[off + 4] as usize & 0x7f) << 21)
                | ((mb[off + 5] as usize & 0x7f) << 14)
                | ((mb[off + 6] as usize & 0x7f) << 7)
                | (mb[off + 7] as usize & 0x7f);
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
        let src = Path::new(r"D:\Drive\Music\网易云歌单\古风戏腔\暗杠、寅子 - 说书人.mp3");
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
}
