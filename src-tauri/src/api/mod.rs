use anyhow::{anyhow, Context, Result};
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE},
    Client, Proxy, StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::store::config::Config;

const API_USER_AGENT: &str =
    "MusicAutoSync/0.1 (Windows; +https://github.com/muxinxy/music-auto-sync)";

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

#[derive(Debug, Clone)]
pub struct LoginStatusResponse {
    pub status: LoginStatus,
    pub meta: ApiResponseMeta,
    pub account_present: bool,
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

#[derive(Debug, Clone, Default)]
pub struct ApiResponseMeta {
    pub duration_ms: u128,
    pub http_status: Option<u16>,
    pub api_code: Option<i64>,
    pub server: Option<String>,
    pub request_id: Option<String>,
    pub retry_after: Option<String>,
}

impl ApiResponseMeta {
    fn from_response(response: &reqwest::Response, duration_ms: u128) -> Self {
        Self {
            duration_ms,
            http_status: Some(response.status().as_u16()),
            server: safe_header(
                response
                    .headers()
                    .get("server")
                    .and_then(|value| value.to_str().ok()),
            ),
            request_id: safe_header(
                response
                    .headers()
                    .get("cf-ray")
                    .or_else(|| response.headers().get("x-request-id"))
                    .and_then(|value| value.to_str().ok()),
            ),
            retry_after: safe_header(
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok()),
            ),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApiCallError {
    pub class: &'static str,
    pub meta: ApiResponseMeta,
}

impl std::fmt::Display for ApiCallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}",
            error_message_from_class(self.class, &self.meta)
        )
    }
}

impl std::error::Error for ApiCallError {}

impl NeteaseApi {
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9"));

        let mut client = Client::builder()
            .default_headers(headers)
            .user_agent(API_USER_AGENT)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30));
        if let Some(proxy_url) = config
            .http_proxy
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            client = client.proxy(Proxy::all(proxy_url).context("HTTP(S) 代理地址无效")?);
        }

        Ok(Self {
            client: client.build()?,
            base: config.api_base.trim_end_matches('/').to_owned(),
            cookie: config.cookie.clone(),
        })
    }

    async fn get_value(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> std::result::Result<(Value, Option<String>, ApiResponseMeta), ApiCallError> {
        let url = format!("{}{}", self.base, path);
        let mut request = self.client.get(&url).query(params);
        if let Some(cookie) = &self.cookie {
            request = request.header("Cookie", cookie);
        }

        let started = Instant::now();
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                return Err(ApiCallError {
                    class: request_error_class(&error),
                    meta: ApiResponseMeta {
                        duration_ms: started.elapsed().as_millis(),
                        ..ApiResponseMeta::default()
                    },
                });
            }
        };
        let status = response.status();
        let mut meta = ApiResponseMeta::from_response(&response, started.elapsed().as_millis());
        let set_cookie = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or(value).to_owned())
            .collect::<Vec<_>>();

        if !status.is_success() {
            let class = http_error_class(status);
            let _ = response.text().await;
            return Err(ApiCallError { class, meta });
        }

        let json = response.json::<Value>().await.map_err(|_| ApiCallError {
            class: "invalid_json",
            meta: meta.clone(),
        })?;
        meta.api_code = json.get("code").and_then(Value::as_i64);
        Ok((
            json,
            (!set_cookie.is_empty()).then(|| set_cookie.join("; ")),
            meta,
        ))
    }

    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<(Value, Option<String>)> {
        let (json, set_cookie, meta) = self
            .get_value(path, params)
            .await
            .map_err(anyhow::Error::new)?;
        ensure_success_code(&json).map_err(|_| {
            anyhow::Error::new(ApiCallError {
                class: "api_business",
                meta,
            })
        })?;
        Ok((json, set_cookie))
    }

    pub async fn login_qr(&self) -> Result<QrSession> {
        let timestamp = cache_buster();
        let (key_json, _) = self
            .get("/login/qr/key", &[("timestamp", timestamp.clone())])
            .await?;
        let key = key_json
            .pointer("/data/unikey")
            .and_then(Value::as_str)
            .context("二维码 key 缺失")?
            .to_owned();
        let (qr_json, _) = self
            .get(
                "/login/qr/create",
                &[
                    ("key", key.clone()),
                    ("qrimg", "true".into()),
                    ("timestamp", cache_buster()),
                ],
            )
            .await?;
        let qr_img = qr_json
            .pointer("/data/qrimg")
            .and_then(Value::as_str)
            .context("二维码图片缺失")?
            .to_owned();
        Ok(QrSession { key, qr_img })
    }

    pub async fn check_qr(
        &self,
        key: &str,
    ) -> std::result::Result<(QrCheckResult, Option<String>, ApiResponseMeta), ApiCallError> {
        let (json, set_cookie, meta) = self
            .get_value(
                "/login/qr/check",
                &[("key", key.into()), ("timestamp", cache_buster())],
            )
            .await?;
        let code = json
            .get("code")
            .and_then(Value::as_i64)
            .ok_or_else(|| ApiCallError {
                class: "invalid_json",
                meta: meta.clone(),
            })?;
        let message = json
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("未知状态")
            .to_owned();
        let state = qr_state_from_code(code)
            .ok_or_else(|| ApiCallError {
                class: "api_business",
                meta: meta.clone(),
            })?
            .to_owned();
        let cookie = json
            .get("cookie")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or(set_cookie);
        if state == "success" && !cookie.as_deref().is_some_and(is_session_cookie) {
            return Err(ApiCallError {
                class: "invalid_session_cookie",
                meta,
            });
        }
        Ok((
            QrCheckResult {
                state,
                message,
                nickname: None,
            },
            cookie,
            meta,
        ))
    }

    pub async fn login_status(&self) -> Result<LoginStatusResponse> {
        let Some(cookie) = self.cookie.as_deref() else {
            return Ok(LoginStatusResponse {
                status: LoginStatus {
                    logged_in: false,
                    nickname: None,
                    user_id: None,
                    avatar_url: None,
                },
                meta: ApiResponseMeta::default(),
                account_present: false,
            });
        };
        let (json, _, meta) = self
            .get_value(
                "/login/status",
                &[("cookie", cookie.to_owned()), ("timestamp", cache_buster())],
            )
            .await
            .map_err(anyhow::Error::new)?;
        ensure_success_code(&json).map_err(|_| {
            anyhow::Error::new(ApiCallError {
                class: "api_business",
                meta: meta.clone(),
            })
        })?;
        let profile = json.pointer("/data/profile");
        let account_present = json
            .pointer("/data/account")
            .is_some_and(|account| !account.is_null());
        Ok(LoginStatusResponse {
            status: LoginStatus {
                logged_in: profile.is_some_and(|profile| !profile.is_null()),
                nickname: profile
                    .and_then(|profile| profile.get("nickname"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                user_id: profile
                    .and_then(|profile| profile.get("userId"))
                    .and_then(Value::as_u64),
                avatar_url: profile
                    .and_then(|profile| profile.get("avatarUrl"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            },
            meta,
            account_present,
        })
    }

    pub async fn user_playlists(&self, user_id: u64, config: &Config) -> Result<Vec<PlaylistInfo>> {
        let (json, _) = self
            .get(
                "/user/playlist",
                &[("uid", user_id.to_string()), ("limit", "1000".into())],
            )
            .await?;
        let settings: HashMap<u64, _> = config
            .playlists
            .iter()
            .map(|playlist| (playlist.id, playlist))
            .collect();
        Ok(json
            .get("playlist")
            .and_then(Value::as_array)
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|playlist| {
                let id = playlist.get("id")?.as_u64()?;
                let setting = settings.get(&id);
                Some(PlaylistInfo {
                    id,
                    name: playlist
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    cover_img_url: playlist
                        .get("coverImgUrl")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    track_count: playlist
                        .get("trackCount")
                        .and_then(Value::as_u64)
                        .unwrap_or_default() as u32,
                    subscribed: playlist
                        .get("subscribed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    enabled: setting.is_some_and(|setting| setting.enabled),
                    last_sync: None,
                    last_result: None,
                })
            })
            .collect())
    }

    pub async fn playlist_tracks(&self, id: u64) -> Result<PlaylistTracks> {
        let mut offset = 0;
        let mut tracks = Vec::new();
        let mut name = String::new();
        loop {
            let (json, _) = self
                .get(
                    "/playlist/track/all",
                    &[
                        ("id", id.to_string()),
                        ("limit", "500".into()),
                        ("offset", offset.to_string()),
                    ],
                )
                .await?;
            if name.is_empty() {
                name = json
                    .pointer("/playlist/name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
            }
            let page: Vec<Track> =
                serde_json::from_value(json.get("songs").cloned().unwrap_or(Value::Array(vec![])))?;
            let count = page.len();
            tracks.extend(page);
            if count < 500 {
                break;
            }
            offset += count;
        }
        Ok(PlaylistTracks { id, name, tracks })
    }

    pub async fn song_url(&self, id: u64, quality: &str) -> Result<SongUrl> {
        let (json, _) = self
            .get(
                "/song/url/v1",
                &[("id", id.to_string()), ("level", quality.to_owned())],
            )
            .await?;
        let data = json
            .get("data")
            .and_then(Value::as_array)
            .and_then(|data| data.first());
        Ok(SongUrl {
            url: data
                .and_then(|item| item.get("url"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            file_type: data
                .and_then(|item| item.get("type"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    pub async fn lyric(&self, id: u64) -> Result<Option<String>> {
        let (json, _) = self.get("/lyric", &[("id", id.to_string())]).await?;
        Ok(json
            .pointer("/lrc/lyric")
            .and_then(Value::as_str)
            .map(str::to_owned))
    }
}

fn ensure_success_code(json: &Value) -> Result<()> {
    let code = json.get("code").and_then(Value::as_i64).unwrap_or(200);
    if code == 200 {
        return Ok(());
    }
    let message = json
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("网易云 API 返回错误");
    Err(anyhow!("API 业务错误 ({code})：{message}"))
}

fn qr_state_from_code(code: i64) -> Option<&'static str> {
    match code {
        800 => Some("expired"),
        801 => Some("waiting"),
        802 => Some("scanned"),
        803 => Some("success"),
        _ => None,
    }
}

fn is_session_cookie(cookie: &str) -> bool {
    cookie.split(';').any(|part| {
        let part = part.trim();
        part.starts_with("MUSIC_U=") || part.starts_with("MUSIC_A=")
    })
}

fn cache_buster() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn safe_header(value: Option<&str>) -> Option<String> {
    value
        .map(|value| {
            value
                .chars()
                .filter(|character| !character.is_control())
                .take(128)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
}

fn request_error_class(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else {
        "request"
    }
}

fn http_error_class(status: StatusCode) -> &'static str {
    match status {
        StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => "http_403",
        StatusCode::TOO_MANY_REQUESTS => "http_429",
        _ if status.is_server_error() => "http_5xx",
        _ => "http",
    }
}

fn error_message_from_class(class: &str, meta: &ApiResponseMeta) -> String {
    let prefix = match class {
        "http_403" => {
            "API 服务拒绝访问。请在设置中更换兼容 API 地址，或为 API 连通性配置 HTTP(S) 代理。"
        }
        "http_429" => "API 服务请求过于频繁，请稍后重试。",
        "http_5xx" => "API 服务暂时不可用，请稍后重试或在设置中更换兼容 API 地址。",
        "timeout" => "连接 API 服务超时，请检查网络、代理或 API 地址。",
        "invalid_json" => "API 服务返回了无法识别的数据。",
        "invalid_session_cookie" => "登录成功但 API 未返回有效会话凭据，请刷新二维码后重试。",
        "api_business" => "API 服务返回业务错误。",
        _ => "无法连接 API 服务，请检查网络、代理或 API 地址。",
    };
    let mut message = match meta.http_status {
        Some(status) => format!("{prefix} (HTTP {status})"),
        None => prefix.to_owned(),
    };
    if let Some(retry_after) = &meta.retry_after {
        message.push_str(&format!("，建议等待 {retry_after} 秒"));
    }
    if let Some(server) = &meta.server {
        message.push_str(&format!("，服务: {server}"));
    }
    if let Some(request_id) = &meta.request_id {
        message.push_str(&format!("，请求标识: {request_id}"));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_all_qr_states() {
        assert_eq!(qr_state_from_code(800), Some("expired"));
        assert_eq!(qr_state_from_code(801), Some("waiting"));
        assert_eq!(qr_state_from_code(802), Some("scanned"));
        assert_eq!(qr_state_from_code(803), Some("success"));
        assert_eq!(qr_state_from_code(200), None);
    }

    #[test]
    fn reports_actionable_forbidden_message() {
        let message = error_message_from_class(
            "http_403",
            &ApiResponseMeta {
                http_status: Some(403),
                server: Some("edge".into()),
                ..ApiResponseMeta::default()
            },
        );
        assert!(message.contains("HTTP 403"));
        assert!(message.contains("更换兼容 API 地址"));
    }

    #[test]
    fn recognizes_netease_session_cookies() {
        assert!(is_session_cookie("MUSIC_U=value; _ntes_nuid=value"));
        assert!(is_session_cookie("MUSIC_A=value"));
        assert!(!is_session_cookie("_ntes_nuid=value"));
    }
}
