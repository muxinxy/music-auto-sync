use regex::Regex;
use std::path::{Path, PathBuf};

use crate::api::Track;

pub fn sanitize_component(input: &str) -> String {
    let invalid = Regex::new(r#"[<>:"/\\|?*\x00-\x1F]"#).unwrap();
    let space = Regex::new(r"\s+").unwrap();
    let mut result = invalid.replace_all(input, "_").to_string();
    result = space
        .replace_all(&result, " ")
        .trim()
        .trim_end_matches('.')
        .to_owned();
    if result.is_empty() {
        "未命名".into()
    } else {
        result
    }
}

pub fn artists(track: &Track) -> String {
    let names = track
        .ar
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    if names.is_empty() {
        "未知歌手".into()
    } else {
        names
    }
}

pub fn apply_template(
    template: &str,
    playlist_name: &str,
    track: &Track,
    position: usize,
) -> String {
    let replacements = [
        ("{歌单名}", playlist_name.to_owned()),
        ("{音轨号}", format!("{:02}", position)),
        ("{歌手}", artists(track)),
        ("{标题}", track.name.clone()),
        ("{专辑}", track.al.name.clone()),
        ("{网易云ID}", track.id.to_string()),
    ];
    let output = replacements
        .into_iter()
        .fold(template.to_owned(), |out, (key, val)| {
            out.replace(key, &sanitize_component(&val))
        });
    sanitize_component(&output)
}

pub fn track_path(
    root: &Path,
    folder_template: &str,
    filename_template: &str,
    playlist_name: &str,
    track: &Track,
    position: usize,
    extension: &str,
) -> PathBuf {
    let folder = apply_template(folder_template, playlist_name, track, position);
    let name = apply_template(filename_template, playlist_name, track, position);
    root.join(folder).join(format!("{}.{}", name, extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_windows_invalid_characters() {
        assert_eq!(sanitize_component("A:B / C?"), "A_B _ C_");
    }
}
