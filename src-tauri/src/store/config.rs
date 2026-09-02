use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const DEFAULT_API_BASE: &str = "https://netease-api.muxinxy.com";

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub api_base: String,
    #[serde(default)]
    pub http_proxy: Option<String>,
    pub music_root: Option<String>,
    pub folder_template: String,
    pub filename_template: String,
    pub quality: String,
    pub auto_sync_on_startup: bool,
    pub sync_interval_minutes: Option<u64>,
    pub ncm_convert: bool,
    #[serde(default)]
    pub ncm_scan_dirs: Vec<String>,
    #[serde(default = "default_true")]
    pub ncm_keep_source: bool,
    pub embed_cover: bool,
    pub embed_lyrics: bool,
    pub write_lrc: bool,
    pub write_m3u8: bool,
    pub concurrency: usize,
    #[serde(default)]
    pub playlists: Vec<PlaylistSyncSetting>,
    pub cookie: Option<String>,
    pub cookie_user: Option<CookieUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSyncSetting {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    pub folder_override: Option<String>,
    pub quality_override: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieUser {
    pub user_id: u64,
    pub nickname: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_base: DEFAULT_API_BASE.into(),
            http_proxy: None,
            music_root: None,
            folder_template: "{歌单名}".into(),
            filename_template: "{歌手} - {标题}".into(),
            quality: "exhigh".into(),
            auto_sync_on_startup: true,
            sync_interval_minutes: Some(60),
            ncm_convert: true,
            ncm_scan_dirs: vec![],
            ncm_keep_source: true,
            embed_cover: true,
            embed_lyrics: false,
            write_lrc: true,
            write_m3u8: true,
            concurrency: 3,
            playlists: vec![],
            cookie: None,
            cookie_user: None,
        }
    }
}

pub fn load(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let data = fs::read_to_string(path)
        .with_context(|| format!("cannot read config {}", path.display()))?;
    serde_json::from_str(&data).context("config.json is malformed")
}

pub fn save(path: &Path, config: &Config) -> Result<()> {
    let data = serde_json::to_vec_pretty(config)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data)?;
    fs::rename(tmp, path).context("cannot atomically replace config.json")
}
