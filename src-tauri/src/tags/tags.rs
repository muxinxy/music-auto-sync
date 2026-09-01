use anyhow::Result;
use lofty::{
    config::WriteOptions,
    file::TaggedFileExt,
    probe::Probe,
    tag::{Accessor, ItemKey, TagExt, TagType},
};
use std::path::Path;

use crate::{api::Track, core::naming::artists};

pub fn write_basic_tags(
    path: &Path,
    track: &Track,
    position: usize,
    netease_id: u64,
) -> Result<()> {
    let mut tagged_file = Probe::open(path)?.read()?;
    let tag_type = tagged_file.primary_tag_type();
    if let Some(tag) = tagged_file.primary_tag_mut() {
        tag.set_title(track.name.clone());
        tag.set_artist(artists(track));
        tag.set_album(track.al.name.clone());
        tag.set_track(position as u32);
        tag.insert_text(ItemKey::Comment, format!("netease-id:{netease_id}"));
        tag.save_to_path(path, WriteOptions::default())?;
    } else {
        let mut tag = lofty::tag::Tag::new(if tag_type == TagType::Id3v2 {
            tag_type
        } else {
            TagType::Id3v2
        });
        tag.set_title(track.name.clone());
        tag.set_artist(artists(track));
        tag.set_album(track.al.name.clone());
        tag.set_track(position as u32);
        tag.insert_text(ItemKey::Comment, format!("netease-id:{netease_id}"));
        tag.save_to_path(path, WriteOptions::default())?;
    }
    Ok(())
}
