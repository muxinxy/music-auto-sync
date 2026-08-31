use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::store::config::Config;

#[derive(Clone)]
pub struct NeteaseApi {
    client: Client,
    base: String,
    cookie: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrSession {
    pub key: String,
    pub qr_img: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrCheckResult {
    pub state: String,
    pub message: String,
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    pub logged_in: bool,
    pub nickname: Option<String>,
    pub user_id: Option<u64>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistInfo {
    pub id: u64,
    pub name: String,
    pub cover_img_url: String,
    pub track_count: u32,
    pub subscribed: bool,
    pub enabled: bool,
    pub last_sync: Option<String>,
    pub last_result: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Track {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub ar: Vec<Artist>,
    #[serde(default)]
    pub al: Album,
    #[serde(default)]
    pub dt: u64,
    #[serde(default)]
    pub no: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Artist {
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Album {
    pub id: u64,
    pub name: String,
    #[serde(rename = "picUrl")]
    pub pic_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlaylistTracks {
    pub id: u64,
    pub name: String,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct SongUrl {
    pub url: Option<String>,
    pub file_type: Option<String>,
}

impl NeteaseApi {
    pub fn from_config(config: &Config) -> Result<Self> {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36")
            .build()?;
        Ok(Self {
            client,
            base: config.api_base.trim_end_matches('/').to_owned(),
            cookie: config.cookie.clone(),
        })
    }

    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<(Value, Option<String>)> {
        let url = format!("{}{}", self.base, path);
        let mut request = self.client.get(url).query(params);
        if let Some(cookie) = &self.cookie {
            request = request.header("Cookie", cookie);
        }
        let response = request.send().await?.error_for_status()?;
        let set_cookie = response.headers().get_all("set-cookie").iter()
            .filter_map(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).to_owned())
            .collect::<Vec<_>>();
        let json = response.json::<Value>().await?;
        if json.get("code").and_then(Value::as_i64).unwrap_or(200) != 200 {
            let message = json.get("message").and_then(Value::as_str).unwrap_or("网易云 API 返回错误").to_owned();
            return Err(anyhow!(message));
        }
        Ok((json, (!set_cookie.is_empty()).then(|| set_cookie.join("; "))))
    }

    pub async fn login_qr(&self) -> Result<QrSession> {
        let (key_json, _) = self.get("/login/qr/key", &[]).await?;
        let key = key_json.pointer("/data/unikey").and_then(Value::as_str)
            .context("二维码 key 缺失")?.to_owned();
        let (qr_json, _) = self.get("/login/qr/create", &[("key", key.clone()), ("qrimg", "true".into())]).await?;
        let qr_img = qr_json.pointer("/data/qrimg").and_then(Value::as_str)
            .context("二维码图片缺失")?.to_owned();
        Ok(QrSession { key, qr_img })
    }

    pub async fn check_qr(&self, key: &str) -> Result<(QrCheckResult, Option<String>)> {
        let (json, cookie) = self.get("/login/qr/check", &[("key", key.into())]).await?;
        let code = json.get("code").and_then(Value::as_i64).unwrap_or_default();
        let message = json.get("message").and_then(Value::as_str).unwrap_or("未知状态").to_owned();
        let state = match code {
            800 => "expired",
            801 => "waiting",
            802 => "scanned",
            803 => "success",
            _ => "waiting",
        }.to_owned();
        Ok((QrCheckResult { state, message, nickname: None }, cookie.or_else(|| json.get("cookie").and_then(Value::as_str).map(str::to_owned))))
    }

    pub async fn login_status(&self) -> Result<LoginStatus> {
        if self.cookie.is_none() {
            return Ok(LoginStatus { logged_in: false, nickname: None, user_id: None, avatar_url: None });
        }
        let (json, _) = self.get("/login/status", &[]).await?;
        let profile = json.pointer("/data/profile");
        Ok(LoginStatus {
            logged_in: profile.is_some_and(|p| !p.is_null()),
            nickname: profile.and_then(|p| p.get("nickname")).and_then(Value::as_str).map(str::to_owned),
            user_id: profile.and_then(|p| p.get("userId")).and_then(Value::as_u64),
            avatar_url: profile.and_then(|p| p.get("avatarUrl")).and_then(Value::as_str).map(str::to_owned),
        })
    }

    pub async fn user_playlists(&self, user_id: u64, config: &Config) -> Result<Vec<PlaylistInfo>> {
        let (json, _) = self.get("/user/playlist", &[("uid", user_id.to_string()), ("limit", "1000".into())]).await?;
        let settings: HashMap<u64, _> = config.playlists.iter().map(|p| (p.id, p)).collect();
        Ok(json.get("playlist").and_then(Value::as_array).unwrap_or(&vec![]).iter().filter_map(|p| {
            let id = p.get("id")?.as_u64()?;
            let setting = settings.get(&id);
            Some(PlaylistInfo {
                id,
                name: p.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                cover_img_url: p.get("coverImgUrl").and_then(Value::as_str).unwrap_or_default().to_owned(),
                track_count: p.get("trackCount").and_then(Value::as_u64).unwrap_or_default() as u32,
                subscribed: p.get("subscribed").and_then(Value::as_bool).unwrap_or(false),
                enabled: setting.is_some_and(|s| s.enabled),
                last_sync: None,
                last_result: None,
            })
        }).collect())
    }

    pub async fn playlist_tracks(&self, id: u64) -> Result<PlaylistTracks> {
        let mut offset = 0;
        let mut tracks = Vec::new();
        let mut name = String::new();
        loop {
            let (json, _) = self.get("/playlist/track/all", &[
                ("id", id.to_string()),
                ("limit", "500".into()),
                ("offset", offset.to_string()),
            ]).await?;
            if name.is_empty() {
                name = json.pointer("/playlist/name").and_then(Value::as_str).unwrap_or_default().to_owned();
            }
            let page: Vec<Track> = serde_json::from_value(json.get("songs").cloned().unwrap_or(Value::Array(vec![])))?;
            let count = page.len();
            tracks.extend(page);
            if count < 500 { break; }
            offset += count;
        }
        Ok(PlaylistTracks { id, name, tracks })
    }

    pub async fn song_url(&self, id: u64, quality: &str) -> Result<SongUrl> {
        let (json, _) = self.get("/song/url/v1", &[("id", id.to_string()), ("level", quality.to_owned())]).await?;
        let data = json.get("data").and_then(Value::as_array).and_then(|d| d.first());
        Ok(SongUrl {
            url: data.and_then(|x| x.get("url")).and_then(Value::as_str).map(str::to_owned),
            file_type: data.and_then(|x| x.get("type")).and_then(Value::as_str).map(str::to_owned),
        })
    }

    pub async fn lyric(&self, id: u64) -> Result<Option<String>> {
        let (json, _) = self.get("/lyric", &[("id", id.to_string())]).await?;
        Ok(json.pointer("/lrc/lyric").and_then(Value::as_str).map(str::to_owned))
    }
}
