use anyhow::{Context, Result};
use lofty::{
    config::WriteOptions,
    file::TaggedFileExt,
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
}
