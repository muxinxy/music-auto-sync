//! 网易云音乐 .ncm 文件解密转换（参考 ncmdump / ncm-to-mp3）。
//!
//! 格式：`CTENFDAM` 头 → 密钥段 → 元数据段 → 封面段 → RC4 加密音频。
//!  - 密钥段：xor 0x64 → AES-128-ECB(CORE_KEY) 解密 → 剥 17 字节
//!    `neteasecloudmusic` 前缀 → RC4 密钥；
//!  - 元数据段：xor 0x63 → 文本 `163 key(Don't modify):<b64>` →
//!    b64 解码 → AES-128-ECB(META_KEY) 解密 → `music:{json}`。
//!    密钥与 163 key / NCM 元数据同款，即 `#14ljk_!\]&0U<('`（见 core::netease_key）。
//!  - 音频：RC4 变体 keybox 流密码，逐字节双查表，与 ncmdump 一致。

use aes::Aes128;
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyInit};
use ecb::Decryptor;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// 密钥段固定 17 字节前缀（AES 解密后剥离）。
const KEY_BOX_PREFIX: &[u8] = b"neteasecloudmusic";

const CORE_KEY: [u8; 16] = [
    0x68, 0x7a, 0x48, 0x52, 0x41, 0x6d, 0x73, 0x6f, 0x35, 0x6b, 0x49, 0x6e, 0x62, 0x61, 0x78, 0x57,
];

/// 元数据段加密密钥：与 163 key 相同（netease_key 已持有并验证）。
/// 这里复用其常量，避免两处维护同一密钥。
const META_KEY: &[u8; 16] = crate::core::netease_key::meta_key();

#[derive(Debug, Clone, Deserialize)]
pub struct NcmMetadata {
    #[serde(rename = "musicName")]
    pub music_name: String,
    pub artist: Vec<(String, u64)>,
    pub album: String,
    #[serde(rename = "albumPic")]
    pub album_pic: Option<String>,
    pub format: String,
}

#[derive(Debug, Clone)]
pub struct NcmOutput {
    pub path: PathBuf,
    pub metadata: NcmMetadata,
}

pub fn convert(input: &Path, output_dir: &Path) -> Result<NcmOutput> {
    let bytes = fs::read(input).with_context(|| format!("cannot read {}", input.display()))?;
    if bytes.len() < 32 || &bytes[..8] != b"CTENFDAM" {
        return Err(anyhow!("not a supported NCM file"));
    }
    let mut offset = 10usize;

    // ── 密钥段 ──
    let key_len = take_u32(&bytes, &mut offset)? as usize;
    let mut key_data = take(&bytes, &mut offset, key_len)?.to_vec();
    xor_all(&mut key_data, 0x64);
    let key_box_plain = aes_decrypt(&key_data, &CORE_KEY)?;
    if key_box_plain.len() <= KEY_BOX_PREFIX.len() {
        return Err(anyhow!("invalid NCM key block"));
    }
    let rc4_key = &key_box_plain[KEY_BOX_PREFIX.len()..];
    let key_stream = make_key_box(rc4_key);

    // ── 元数据段（可能为空，老文件无元数据也能转）──
    let meta_len = take_u32(&bytes, &mut offset)? as usize;
    let mut metadata = None;
    if meta_len > 0 {
        let mut meta_data = take(&bytes, &mut offset, meta_len)?.to_vec();
        xor_all(&mut meta_data, 0x63);
        let meta_text = String::from_utf8_lossy(&meta_data);
        let encrypted_meta = meta_text
            .strip_prefix(crate::core::netease_key::KEY_PREFIX)
            .context("invalid NCM metadata")?;
        let mut meta_bytes = base64::engine::general_purpose::STANDARD
            .decode(encrypted_meta)
            .context("invalid NCM metadata base64")?;
        // 个别文件密文非 16 倍数时截断到 16 倍数（与 163 key 读取一致）。
        let usable = meta_bytes.len() - (meta_bytes.len() % 16);
        meta_bytes.truncate(usable);
        if usable >= 16 {
            let json_text = String::from_utf8(aes_decrypt(&meta_bytes, META_KEY)?)
                .context("NCM metadata not UTF-8")?;
            let json_text = json_text.strip_prefix("music:").unwrap_or(&json_text);
            metadata = Some(
                serde_json::from_str::<NcmMetadata>(json_text)
                    .context("invalid NCM metadata JSON")?,
            );
        }
    }
    let metadata = metadata.context("NCM file has no metadata")?;

    // ── 跳过 CRC 与 gap（4+5 字节）──
    offset += 9;

    // ── 封面段 ──
    let image_len = take_u32(&bytes, &mut offset)? as usize;
    if image_len > 0 {
        let _ = take(&bytes, &mut offset, image_len)?;
    }

    // ── 音频段：RC4 变体流密码 ──
    let audio = &bytes[offset..];
    let mut decoded = Vec::with_capacity(audio.len());
    // 与 ncmdump 一致：j = (i + 1) & 0xff，逐字节：
    // plain[i] = enc[i] ^ box[ (box[j] + box[ (box[j] + j) & 0xff ]) & 0xff ]
    for (i, byte) in audio.iter().enumerate() {
        let j = (i + 1) & 0xff;
        let kj = key_stream[j] as usize;
        let key = (key_stream[(kj + j) & 0xff] as usize + kj) & 0xff;
        decoded.push(byte ^ key_stream[key]);
    }

    fs::create_dir_all(output_dir)?;
    // 输出名跟随源 .ncm 文件名（去 .ncm 后缀换实际格式），保持与原文件一致；
    // 不用 NCM 内嵌 musicName，避免“李荣浩 - 年少有为.ncm”转出“年少有为.mp3”的错位。
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&metadata.music_name);
    let file_name = format!("{}.{}", sanitize(stem), metadata.format);
    let target = unique_path(output_dir.join(file_name));
    fs::write(&target, decoded)?;
    Ok(NcmOutput {
        path: target,
        metadata,
    })
}

fn take<'a>(data: &'a [u8], offset: &mut usize, length: usize) -> Result<&'a [u8]> {
    let end = offset.checked_add(length).context("NCM data overflow")?;
    let slice = data.get(*offset..end).context("truncated NCM file")?;
    *offset = end;
    Ok(slice)
}

fn take_u32(data: &[u8], offset: &mut usize) -> Result<u32> {
    let bytes = take(data, offset, 4)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn aes_decrypt(data: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    let mut buffer = data.to_vec();
    Decryptor::<Aes128>::new(key.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map(|data| data.to_vec())
        .map_err(|_| anyhow!("invalid NCM AES padding"))
}

fn xor_all(data: &mut [u8], value: u8) {
    for byte in data {
        *byte ^= value;
    }
}

/// 构建 RC4 变体 keybox（与 ncmdump 一致：状态只在 KSA 时置换，解流过程不改 box）。
fn make_key_box(key: &[u8]) -> [u8; 256] {
    let mut box_ = [0u8; 256];
    for (i, value) in box_.iter_mut().enumerate() {
        *value = i as u8;
    }
    let mut j = 0usize;
    for i in 0..256 {
        j = (box_[i] as usize + j + key[i % key.len()] as usize) & 0xff;
        box_.swap(i, j);
    }
    box_
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect()
}

fn unique_path(mut path: PathBuf) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|x| x.to_str())
        .unwrap_or("track")
        .to_owned();
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("mp3")
        .to_owned();
    let mut counter = 2;
    while path.exists() {
        path.set_file_name(format!("{} ({counter}).{ext}", stem));
        counter += 1;
    }
    path
}

/// 单个 .ncm 转换的结果。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NcmConvertItemResult {
    pub source: String,
    pub output: Option<String>,
    pub status: String, // converted | skipped | failed
    pub error: Option<String>,
}

/// 把单个 .ncm 文件转换到其同目录：写 `.ncm.converted.json` 标记；keep_source=false 时删源；
/// overwrite=true 时即使已有标记也重转。返回结构化结果（不抛错）。
pub fn convert_file_with_marker(
    input: &Path,
    keep_source: bool,
    overwrite: bool,
) -> NcmConvertItemResult {
    let source = input.to_string_lossy().into_owned();
    let marker = input.with_extension("ncm.converted.json");
    if marker.exists() && !overwrite {
        return NcmConvertItemResult {
            source,
            output: None,
            status: "skipped".into(),
            error: None,
        };
    }
    let output_dir = match input.parent() {
        Some(dir) => dir.to_path_buf(),
        None => {
            return NcmConvertItemResult {
                source,
                output: None,
                status: "failed".into(),
                error: Some("no parent directory".into()),
            };
        }
    };
    match convert(input, &output_dir) {
        Ok(output) => {
            let marker_json = serde_json::json!({
                "source": input.to_string_lossy(),
                "output": output.path.to_string_lossy(),
                "convertedAt": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                "format": output.metadata.format,
            });
            if let Err(error) = fs::write(&marker, serde_json::to_vec_pretty(&marker_json).unwrap_or_default()) {
                return NcmConvertItemResult {
                    source,
                    output: Some(output.path.to_string_lossy().into_owned()),
                    status: "converted".into(),
                    error: Some(format!("marker write failed: {error}")),
                };
            }
            if !keep_source && input.is_file() {
                let _ = fs::remove_file(input);
            }
            NcmConvertItemResult {
                source,
                output: Some(output.path.to_string_lossy().into_owned()),
                status: "converted".into(),
                error: None,
            }
        }
        Err(error) => NcmConvertItemResult {
            source,
            output: None,
            status: "failed".into(),
            error: Some(error.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实文件（华语高手/李荣浩 - 年少有为.ncm）应能解出元数据 + 合法音频头。
    /// 测试机才存在该目录；不存在时跳过。
    #[test]
    fn converts_real_ncm_file() {
        let input = Path::new(
            r"D:\Drive\Music\网易云歌单\华语高手\李荣浩 - 年少有为.ncm",
        );
        if !input.is_file() {
            eprintln!("skipping: real NCM file not present");
            return;
        }
        let out_dir = std::env::temp_dir().join("ncm-test-music-auto-sync");
        let _ = fs::remove_dir_all(&out_dir);
        fs::create_dir_all(&out_dir).unwrap();
        let output = convert(input, &out_dir).expect("convert real ncm");
        assert_eq!(output.metadata.music_name, "年少有为");
        assert_eq!(output.metadata.format, "mp3");
        // 输出名跟随源 .ncm 文件名，而不是 NCM 内嵌歌名。
        assert_eq!(
            output.path.file_name().and_then(|s| s.to_str()),
            Some("李荣浩 - 年少有为.mp3")
        );
        let bytes = fs::read(&output.path).unwrap();
        // 解出的音频应为合法 ID3/MP3 头。
        assert_eq!(&bytes[..3], b"ID3", "decoded audio must start with ID3");
        fs::remove_dir_all(&out_dir).ok();
    }
}
