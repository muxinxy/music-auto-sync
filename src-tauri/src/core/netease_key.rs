//! 网易云音乐 `163 key(Don't modify):` 的解密与（官方等价格式）加密。
//!
//! 官方网易客户端下载音频时会把歌曲元数据以
//! `music:{...}` JSON → AES-128-ECB(密钥 `#14ljk_!\]&0U<('`) → Base64 的形式
//! 写入音频标签的 COMM(备注) 帧，前缀 `163 key(Don't modify):`。
//! 本模块：
//!  - `decrypt_music_id`：从标签文本解出 musicId（用于本地文件与歌单曲目的精确匹配）；
//!  - `encrypt_official`：把下载到的歌曲元数据按官方字段结构加密成同款 163 key，
//!    供软件下载时写入，保证自家可回读且结构与官方一致。
//!
//! 算法经真实文件验证：华语情歌 13/15 个带 key 文件解出 musicId 且命中歌单。

use aes::Aes128;
use base64::Engine;
use cipher::{
    block_padding::{NoPadding, Pkcs7},
    BlockDecryptMut, BlockEncryptMut, KeyInit,
};
use ecb::{Decryptor, Encryptor};

/// 网易 163 key 的固定 AES-128 密钥（与 NCM 元数据密钥相同）。
/// 字节即 `#14ljk_!\]&0U<('`（0x5c 为单个反斜杠，用数组字面量杜绝转义歧义）。
const META_KEY: &[u8; 16] = &[
    0x23, 0x31, 0x34, 0x6c, 0x6a, 0x6b, 0x5f, 0x21, 0x5c, 0x5d, 0x26, 0x30, 0x55, 0x3c, 0x27,
    0x28,
];
/// 163 key 前缀。
pub const KEY_PREFIX: &str = "163 key(Don't modify):";

/// 供写 163 key 的歌曲元数据（与官方 music JSON 字段一一对应）。
#[derive(Debug, Clone)]
pub struct OfficialKeyMeta {
    pub music_id: u64,
    pub music_name: String,
    /// 歌手 [(名字, 网易歌手 id)]，官方格式为 [[name, id], ...]。
    pub artists: Vec<(String, u64)>,
    pub album: String,
    pub album_id: u64,
    pub album_pic_doc_id: Option<String>,
    pub album_pic: Option<String>,
    /// 比特率（bps），官方用下载音质的 br。
    pub bitrate: Option<u64>,
    /// 资源标识：官方为 mp3DocId；本地以下载响应的 md5 填充（格式一致）。
    pub mp3_doc_id: Option<String>,
    pub duration: u64,
    pub mv_id: u64,
    pub format: String,
}

/// 从标签文本中解析网易歌曲 id：
///  1. 优先 `163 key(Don't modify):<b64>` → AES 解密 → 提 musicId
///  2. 兼容旧格式 `netease-id:<n>`（早期版本写入）
pub fn parse_music_id(comment_text: &str) -> Option<u64> {
    if let Some(key) = extract_key_value(comment_text) {
        if let Some(id) = decrypt_music_id(&key) {
            return Some(id);
        }
    }
    // 兼容早期 `netease-id:<id>` 纯文本。
    for line in comment_text.split(['\0', '\n', ';']) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("netease-id:") {
            if let Ok(id) = rest.trim().parse::<u64>() {
                return Some(id);
            }
        }
    }
    None
}

/// 从任意文本里提取 `163 key(Don't modify):<b64>` 的 b64 部分。
fn extract_key_value(text: &str) -> Option<String> {
    let idx = text.find(KEY_PREFIX)?;
    let rest = &text[idx + KEY_PREFIX.len()..];
    // 取连续的 base64 字符。
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='))
        .unwrap_or(rest.len());
    let b64 = &rest[..end];
    if b64.is_empty() {
        return None;
    }
    Some(b64.to_owned())
}

/// AES-128-ECB 解密 + 去 PKCS7。真实文件 padding 可能不标准（来源截断），
/// Pkcs7 严格校验失败时回退 NoPadding（不去尾），再手动剥合法 PKCS7 尾。
fn aes_decrypt(data: &[u8]) -> Option<Vec<u8>> {
    // 主路径：标准 PKCS7。
    let decryptor = Decryptor::<Aes128>::new(META_KEY.into());
    if let Ok(plain) = decryptor.decrypt_padded_vec_mut::<Pkcs7>(data) {
        return Some(plain);
    }
    // 回退：NoPadding 整段解，再手动剥 PKCS7（若末尾恰好是合法填充）。
    let decryptor = Decryptor::<Aes128>::new(META_KEY.into());
    let mut plain = decryptor.decrypt_padded_vec_mut::<NoPadding>(data).ok()?;
    let pad = *plain.last()? as usize;
    if (1..=16).contains(&pad) && pad <= plain.len() {
        let tail = &plain[plain.len() - pad..];
        if tail.iter().all(|b| *b as usize == pad) {
            plain.truncate(plain.len() - pad);
        }
    }
    Some(plain)
}

/// AES-128-ECB + PKCS7 加密（官方同款）。
fn aes_encrypt(data: &[u8]) -> Vec<u8> {
    Encryptor::<Aes128>::new(META_KEY.into())
        .encrypt_padded_vec_mut::<Pkcs7>(data)
}

/// 解 163 key：b64 解码 → 取前 16 倍数字节 → AES 解密 → 提取 musicId。
/// 官方/自家写入的密文为 16 倍数；个别来源末尾可能带残留字节，按 16 倍数截取。
pub fn decrypt_music_id(key_b64: &str) -> Option<u64> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(key_b64.trim())
        .ok()?;
    let usable = data.len() - (data.len() % 16);
    if usable < 32 {
        return None;
    }
    let plain = aes_decrypt(&data[..usable])?;
    let text = String::from_utf8_lossy(&plain);
    extract_music_id(&text)
}

/// 从解密文本（`music:{...}` 或裸 JSON）中提取 musicId。
fn extract_music_id(text: &str) -> Option<u64> {
    // 找 `"musicId":N`
    let idx = text.find("\"musicId\"")?;
    let after = &text[idx + "\"musicId\"".len()..];
    let after_colon = after.strip_prefix(':')?.trim_start();
    let num: String = after_colon
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if num.is_empty() {
        return None;
    }
    num.parse::<u64>().ok()
}

/// 把歌曲元数据按官方结构加密成 `163 key(Don't modify):<b64>`。
/// 字段顺序与官方一致（musicId, musicName, artist[[name,id]], album, albumId,
/// albumPicDocId, albumPic, bitrate, mp3DocId, duration, mvId, alias, transNames, format）。
pub fn encrypt_official(meta: &OfficialKeyMeta) -> String {
    let mut obj = serde_json::Map::new();
    obj.insert("musicId".into(), serde_json::json!(meta.music_id));
    obj.insert("musicName".into(), serde_json::json!(meta.music_name));
    obj.insert(
        "artist".into(),
        serde_json::json!(meta
            .artists
            .iter()
            .map(|(name, id)| serde_json::json!([name, id]))
            .collect::<Vec<_>>()),
    );
    obj.insert("album".into(), serde_json::json!(meta.album));
    obj.insert("albumId".into(), serde_json::json!(meta.album_id));
    obj.insert(
        "albumPicDocId".into(),
        serde_json::json!(meta.album_pic_doc_id.clone().unwrap_or_default()),
    );
    obj.insert(
        "albumPic".into(),
        serde_json::json!(meta.album_pic.clone().unwrap_or_default()),
    );
    obj.insert(
        "bitrate".into(),
        serde_json::json!(meta.bitrate.unwrap_or(0)),
    );
    obj.insert(
        "mp3DocId".into(),
        serde_json::json!(meta.mp3_doc_id.clone().unwrap_or_default()),
    );
    obj.insert("duration".into(), serde_json::json!(meta.duration));
    obj.insert("mvId".into(), serde_json::json!(meta.mv_id));
    obj.insert("alias".into(), serde_json::json!([]));
    obj.insert("transNames".into(), serde_json::json!([]));
    obj.insert("format".into(), serde_json::json!(meta.format));
    let json_text = format!("music:{}", serde_json::Value::Object(obj));
    let cipher = aes_encrypt(json_text.as_bytes());
    let b64 = base64::engine::general_purpose::STANDARD.encode(cipher);
    format!("{KEY_PREFIX}{b64}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实文件样本：G.E.M.邓紫棋 - 画 (Live Piano Session II) 的 163 key。
    const REAL_KEY_HUA: &str = "L64FU3W4YxX3ZFTmbZ+8/WkGGwzGmhtkqUdCmi/f4tFDC6tjyE/trzM8Rs6UpSxCWF6rIPK+RXOS+vxruvMXI+s9uNv50q0eqsvJBKMdfPdptPdkBPWHuWuFLSKv4kXEP6gr/NppjPy/4CbsmeQ19VfWnCph7BpfPRipYpJXN0PVwFALdbuPZAd538oKztRClUri9d7/wDOuHswvEoc+V6Eo837cpUM/NYV9ktFJoscgjCNlm0tTq8F0bsiUON9Zdt/12f4JEoDXZ3bt/7Ysqvxd";

    /// 真实文件样本：柏松 - 世间美好与你环环相扣（完整 416 字节官方 key）。
    const REAL_KEY_SHIJIAN: &str = "L64FU3W4YxX3ZFTmbZ+8/b2vC7WyJ7EEQRKWcpdFkHW+U5ywwfaIEtmHrDCGe+EaITaFVY4rwOCHPwOrkln0Gzs/et6AYszMX9Rm/G7omnRXAy2SIKgcDlMKhf541tQkMscwe5j1l84TIpUkY09eMk90PQJa5h31BhrbSM9BPzb/by7L/H2l+k5icl6NUTadGionXYMSuzqlEvtUoslNFVeO+EY7VrDtGniz4OuJOgvRaQ38LHygVlodJG3PsvpEKMO1dVGiD0qHZihQM+XZO0VJvJOM+sQz12YlrqqN5kRssLmkMXQyh3x8NFAN8xO/I3s89x0vq+dV19uwjGuMB7RtrusAf5y7C/hcpcY91+t8I090jH+fbgHMSoPVbD/qaUYyfA5DQhCXsX+Z+X1Zt8ywYC8JEvQRS/WYkdPVxAVf5VafcFtJMi2TSAzfh+p4SN6h6aVxh1dHEZHPqzfAWkbUGqB2YlmmIifNnYSn+AKWhpz1au0EOQC7gp6gUKZmPH8fqYlzgOgb5XRhFqAy29A3+W425B3qGYe4us834g8=";

    #[test]
    fn decrypts_real_world_keys() {
        assert_eq!(decrypt_music_id(REAL_KEY_HUA), Some(412911436));
        assert_eq!(decrypt_music_id(REAL_KEY_SHIJIAN), Some(1363948882));
    }

    #[test]
    fn extracts_from_full_comment_text() {
        let comment = format!("XXX{KEY_PREFIX}{REAL_KEY_HUA} trailing");
        assert_eq!(parse_music_id(&comment), Some(412911436));
    }

    #[test]
    fn parses_legacy_netease_id_text() {
        assert_eq!(parse_music_id("netease-id:412911436"), Some(412911436));
        assert_eq!(parse_music_id("netease-id: 12345"), Some(12345));
    }

    #[test]
    fn encrypt_roundtrip_matches_official_prefix() {
        let meta = OfficialKeyMeta {
            music_id: 412911436,
            music_name: "画 (Live Piano Session II)".into(),
            artists: vec![("G.E.M.邓紫棋".into(), 7763)],
            album: "再见".into(),
            album_id: 34678769,
            album_pic_doc_id: Some("1410673427960641".into()),
            album_pic: Some("https://p3.music.126.net/x/1410673427960641.jpg".into()),
            bitrate: Some(320000),
            mp3_doc_id: Some("dfe64923ceda904ce0f4efaee8f75c07".into()),
            duration: 168739,
            mv_id: 0,
            format: "mp3".into(),
        };
        let key = encrypt_official(&meta);
        assert!(key.starts_with(KEY_PREFIX));
        // 自家能解回同一 musicId。
        let b64 = key.trim_start_matches(KEY_PREFIX);
        assert_eq!(decrypt_music_id(b64), Some(412911436));
    }
}
