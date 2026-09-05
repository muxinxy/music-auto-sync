use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const DEFAULT_API_BASE: &str = "https://netease-api.muxinxy.com";

fn default_true() -> bool {
    true
}

fn default_artist_separator() -> String {
    "、".into()
}

fn default_language() -> String {
    "zh-CN".into()
}

fn default_ua() -> String {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36".into()
}

fn default_preflight() -> bool {
    true
}

fn default_retry() -> usize {
    3
}

fn default_download_source() -> String {
    "auto".into()
}

fn default_upload_manual() -> bool {
    false
}

fn default_sync_mode() -> String {
    "mirror".into()
}

fn default_theme() -> String {
    "system".into()
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
    #[serde(default = "default_artist_separator")]
    pub artist_separator: String,
    #[serde(default = "default_language")]
    pub language: String,
    /// 界面主题：system / light / dark。
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_ua")]
    pub ua: String,
    #[serde(default = "default_preflight")]
    pub preflight: bool,
    #[serde(default = "default_retry")]
    pub retry: usize,
    pub quality: String,
    #[serde(default = "default_download_source")]
    pub download_source: String,
    /// 同步模式（全局默认）：mirror=镜像, add_only=仅新增, delete_only=仅删除（作用于歌单→本地下载侧）。
    #[serde(default = "default_sync_mode")]
    pub sync_mode: String,
    /// 是否把“手动放入歌单文件夹的本地音频”补进网易歌单（仅新增，绝不反向删歌；需我创建的歌单）。
    #[serde(default = "default_upload_manual")]
    pub upload_manual: bool,
    pub auto_sync_on_startup: bool,
    pub sync_interval_minutes: Option<u64>,
    /// 开机自启（Windows 注册表 HKCU\...\Run）。
    #[serde(default)]
    pub auto_launch: bool,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default = "default_true")]
    pub use_random_cn_ip: bool,
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
    /// 覆盖全局同步模式（mirror/add_only/delete_only）。
    #[serde(default)]
    pub mode_override: Option<String>,
    /// 覆盖全局“补录手动放入的歌到网易歌单”开关。
    #[serde(default)]
    pub upload_manual: Option<bool>,
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
            artist_separator: "、".into(),
            language: "zh-CN".into(),
            theme: "system".into(),
            ua: default_ua(),
            preflight: true,
            retry: 3,
            quality: "exhigh".into(),
            download_source: "auto".into(),
            sync_mode: "mirror".into(),
            upload_manual: false,
            auto_sync_on_startup: false,
            sync_interval_minutes: None,
            auto_launch: false,
            close_to_tray: true,
            use_random_cn_ip: false,
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
