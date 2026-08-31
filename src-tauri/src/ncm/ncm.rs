use aes::Aes128;
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyInit};
use ecb::Decryptor;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const CORE_KEY: &[u8; 16] = b"hzHRAmso5kInbaxW";
const META_KEY: &[u8; 16] = b"2331kjk2k3k4k5k6";

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
    let key_len = take_u32(&bytes, &mut offset)? as usize;
    let mut key_data = take(&bytes, &mut offset, key_len)?.to_vec();
    xor_all(&mut key_data, 0x64);
    let key_box = aes_decrypt(&key_data, CORE_KEY)?;
    if key_box.len() <= 17 { return Err(anyhow!("invalid NCM key block")); }
    let key = &key_box[17..];
    let key_stream = make_key_box(key);

    let meta_len = take_u32(&bytes, &mut offset)? as usize;
    let mut meta_data = take(&bytes, &mut offset, meta_len)?.to_vec();
    xor_all(&mut meta_data, 0x63);
    let meta_text = String::from_utf8(aes_decrypt(&meta_data, META_KEY)?)?;
    let encrypted_meta = meta_text.strip_prefix("163 key(Don't modify):").context("invalid NCM metadata")?;
    let mut meta_bytes = base64::engine::general_purpose::STANDARD.decode(encrypted_meta)?;
    xor_all(&mut meta_bytes, 0x63);
    let json_text = String::from_utf8(aes_decrypt(&meta_bytes, META_KEY)?)?;
    let metadata: NcmMetadata = serde_json::from_str(json_text.strip_prefix("music:").unwrap_or(&json_text))?;

    let image_len = take_u32(&bytes, &mut offset)? as usize;
    let _image = take(&bytes, &mut offset, image_len)?;
    let audio = &bytes[offset..];
    let mut decoded = Vec::with_capacity(audio.len());
    for (i, byte) in audio.iter().enumerate() {
        decoded.push(byte ^ key_stream[(i + 1) & 0xff]);
    }

    fs::create_dir_all(output_dir)?;
    let file_name = format!("{}.{}", sanitize(&metadata.music_name), metadata.format);
    let target = unique_path(output_dir.join(file_name));
    fs::write(&target, decoded)?;
    Ok(NcmOutput { path: target, metadata })
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
    for byte in data { *byte ^= value; }
}

fn make_key_box(key: &[u8]) -> [u8; 256] {
    let mut box_ = [0u8; 256];
    for (i, value) in box_.iter_mut().enumerate() { *value = i as u8; }
    let mut j = 0usize;
    for i in 0..256 {
        j = (box_[i] as usize + j + key[i % key.len()] as usize) & 0xff;
        box_.swap(i, j);
    }
    box_
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect()
}

fn unique_path(mut path: PathBuf) -> PathBuf {
    let stem = path.file_stem().and_then(|x| x.to_str()).unwrap_or("track").to_owned();
    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("mp3").to_owned();
    let mut counter = 2;
    while path.exists() {
        path.set_file_name(format!("{} ({counter}).{ext}", stem));
        counter += 1;
    }
    path
}
