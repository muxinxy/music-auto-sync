//! ID3 标签标题/艺术家 → 网易曲目的匹配（预览与同步复用）。
//!
//! 匹配顺序中的“标签”层：本地音频若有标题/艺术家标签（读自 ID3/Vorbis 等），
//! 用归一化后的标题与歌单曲目名比对，歌手做交叉校验，避免同名不同歌手误配。
//! 规则沿用真实目录校准：歌名完全一致时，歌手一致或歌名较长(>=4)即接受
//! （覆盖发行方≠演唱者场景）；仅“包含”时要求歌手一致或被包含方较长。

use crate::api::Track;

/// 全角字符转半角（常见于中文标点/数字）。
fn fullwidth_to_halfwidth(c: char) -> char {
    match c {
        '　' => ' ',
        '！' => '!',
        '？' => '?',
        '：' => ':',
        '；' => ';',
        '，' => ',',
        '。' => '.',
        '（' => '(',
        '）' => ')',
        '【' => '[',
        '】' => ']',
        '·' => ' ',
        '、' => ' ',
        '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
        _ => c,
    }
}

/// 是否是匹配时忽略的空白/标点。
fn is_ignored(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '&' | '\''
                | '’'
                | '"'
                | '`'
                | '-'
                | '_'
                | '.'
                | ','
                | '/'
                | '\\'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '!'
                | '?'
                | ':'
                | ';'
                | '·'
                | '、'
                | '*'
                | '#'
                | '@'
                | '~'
                | '^'
                | '|'
                | '+'
                | '='
                | '<'
                | '>'
                | '%'
                | '$'
        )
}

/// 把标题归一化为可比较串：小写、去空白与常见标点。
/// 括号内容保留（本地“画 (Live Piano Session II)”与网易同名曲都保留才能互配）。
pub fn normalize_title(s: &str) -> String {
    s.chars()
        .map(fullwidth_to_halfwidth)
        .filter(|c| !is_ignored(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// 歌手名归一化（对比专用）：剥括号注释（如“双笙（陈元汐）”→“双笙”）后小写去标点。
fn normalize_artist(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' | '（' | '【' | '[' => depth += 1,
            ')' | '）' | '】' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => {
                for c in ch
                    .to_string()
                    .chars()
                    .map(fullwidth_to_halfwidth)
                    .filter(|c| !is_ignored(*c))
                {
                    out.extend(c.to_lowercase());
                }
            }
            _ => {}
        }
    }
    out
}

/// 本地歌手串与网易 ar（“/”分隔）是否有交集。本地可按 `,，/&` 分隔。
fn artists_intersect(local_artist: &str, netease_ar: &str) -> bool {
    let local: Vec<String> = local_artist
        .split([' ', ',', '，', '/', '&'])
        .filter(|s| !s.is_empty())
        .map(normalize_artist)
        .filter(|s| !s.is_empty())
        .collect();
    let netease: Vec<String> = netease_ar.split('/').map(normalize_artist).collect();
    local.iter().any(|a| netease.iter().any(|b| a == b))
}

/// 曲目展示用歌手串（网易 ar 拼接，兼容多歌手）。
pub fn track_artists(track: &Track) -> String {
    track
        .ar
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join("/")
}

/// 判断本地标签标题+艺术家是否与某网易曲目匹配。
/// `tag_title`/`tag_artist` 来自音频 ID3/Vorbis 标签（可空）。
/// 规则：标题归一相等（歌手一致或本地无歌手或歌名>=4），或标题互相包含
/// （歌手一致，或被包含方>=4 字）。
pub fn tag_matches_track(tag_title: &str, tag_artist: &str, track: &Track) -> bool {
    let norm_file = normalize_title(tag_title);
    let norm_track = normalize_title(&track.name);
    if norm_file.is_empty() || norm_track.is_empty() {
        return false;
    }
    let track_ar = track_artists(track);
    let artist_match = !tag_artist.is_empty() && artists_intersect(tag_artist, &track_ar);

    if norm_file == norm_track {
        // 歌名完全一致：本地无歌手、歌手一致、或歌名较长(>=4，发行方≠演唱者)均接受。
        if tag_artist.is_empty() || artist_match {
            return true;
        }
        return norm_file.chars().count() >= 4;
    }

    // 包含关系：短者完整出现在长者中，被包含方 >= 2 字。
    let (shorter, longer) = if norm_file.len() <= norm_track.len() {
        (norm_file.as_str(), norm_track.as_str())
    } else {
        (norm_track.as_str(), norm_file.as_str())
    };
    if shorter.chars().count() < 2 || !longer.contains(shorter) {
        return false;
    }
    // 歌手一致 → 接受；否则要求被包含方较长（>=4），防 2-3 字短名跨歌手误配。
    if !tag_artist.is_empty() && artist_match {
        return true;
    }
    shorter.chars().count() >= 4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: u64, name: &str, artists: &[&str]) -> Track {
        Track {
            id,
            name: name.into(),
            ar: artists
                .iter()
                .map(|a| crate::api::Artist { name: a.to_string() })
                .collect(),
            al: Default::default(),
            dt: 0,
            no: 0,
        }
    }

    #[test]
    fn normalizes_titles() {
        assert_eq!(normalize_title("会不会（吉他版）"), "会不会吉他版");
        assert_eq!(normalize_title("Five Hundred Miles"), "fivehundredmiles");
        assert_eq!(
            normalize_title("画 (Live Piano Session II)"),
            "画livepianosessionii"
        );
        assert_eq!(normalize_title("再见（good bye）"), "再见goodbye");
    }

    #[test]
    fn matches_tag_title_artist_from_real_data() {
        // 华语情歌真实标签（于果/侧脸）。
        assert!(tag_matches_track("侧脸", "于果", &track(534542079, "侧脸", &["于果"])));
        // 再见（good bye）: 标签标题可能带括号 → 归一含括号。
        assert!(tag_matches_track(
            "再见（good bye）",
            "G.E.M.邓紫棋",
            &track(36024806, "再见（good bye）", &["G.E.M.邓紫棋"])
        ));
        // 多歌手：标签艺术家“徐秉龙/沈以诚”命中 ar。
        assert!(tag_matches_track(
            "白羊",
            "徐秉龙/沈以诚",
            &track(514761281, "白羊", &["徐秉龙", "沈以诚"])
        ));
        // 歌手带括号（双笙（陈元汐））归一后与“双笙”互配。
        assert!(tag_matches_track(
            "我的一个道姑朋友",
            "双笙",
            &track(1367452194, "我的一个道姑朋友", &["双笙（陈元汐）"])
        ));
        // 发行方 ≠ 演唱者、歌名>=4 → 接受。
        assert!(tag_matches_track(
            "你是人间四月天",
            "邵帅（解忧邵帅）",
            &track(1344897943, "你是人间四月天", &["邵帅"])
        ));
        // 伍佰 & China Blue 交集。
        assert!(tag_matches_track(
            "挪威的森林",
            "伍佰 & China Blue",
            &track(157288, "挪威的森林", &["伍佰 & China Blue"])
        ));
    }

    #[test]
    fn rejects_short_name_wrong_artist() {
        // 2 字名 + 歌手不一致 → 拒绝（防误配）。
        assert!(!tag_matches_track("爱河", "神马乐团", &track(381433, "爱河", &["许云上"])));
        // 歌手不一致拒绝。
        assert!(!tag_matches_track("侧脸", "周杰伦", &track(534542079, "侧脸", &["于果"])));
        // 空标题不匹配。
        assert!(!tag_matches_track("", "于果", &track(534542079, "侧脸", &["于果"])));
    }
}
