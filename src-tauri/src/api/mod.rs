use anyhow::{anyhow, Context, Result};
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE},
    Client, Proxy, StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub mod cache;

use crate::error::UiMessage;
use crate::store::config::Config;

use self::cache::ApiCache;

/// 可缓存端点的 TTL（进程内，重启即失效）。
/// 仅用于低频变化的元数据；易变数据绝不缓存，见各方法内注释。
const CACHE_TTL_PLAYLIST: Duration = Duration::from_secs(5 * 60);
const CACHE_TTL_ACCOUNT: Duration = Duration::from_secs(10 * 60);
const CACHE_TTL_LYRIC: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
pub struct NeteaseApi {
    client: Client,
    base: String,
    cookie: Option<String>,
    random_cn_ip: bool,
    download_source: String,
    /// 进程内 TTL 响应缓存（多实例间 Arc 共享）。同步/写路径用 fresh 实例 = 空缓存。
    api_cache: Arc<ApiCache>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistInfo {
    pub id: u64,
    pub name: String,
    pub cover_img_url: String,
    pub track_count: u32,
    pub subscribed: bool,
    /// 歌单创建者 userId（用于区分“我创建的” vs “我收藏的”）。
    pub creator_user_id: Option<u64>,
    pub enabled: bool,
    pub synced: u32,
    pub overwrite: bool,
    pub last_sync: Option<String>,
    pub last_result: Option<String>,
    /// 该歌单的同步模式覆盖（来自 config.playlists；None = 跟随全局）。
    pub mode_override: Option<String>,
    /// 该歌单的“补录手动放入的歌”开关覆盖（None = 跟随全局）。
    pub upload_manual: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Artist {
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Album {
    pub id: u64,
    pub name: String,
    #[serde(rename = "picUrl")]
    pub pic_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTracks {
    pub id: u64,
    pub name: String,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct SongUrl {
    pub url: Option<String>,
    pub file_type: Option<String>,
    /// 服务端实际返回的音质等级（/song/url/v1 的 level 字段）。
    pub level: Option<String>,
    /// 服务端返回的文件大小（字节），可用于识别试听片段。
    pub size: Option<u64>,
    /// 服务端返回的比特率（br 字段，bps），用于写官方 163 key 的 bitrate。
    pub br: Option<u64>,
    /// 服务端返回的资源 md5（32 位 hex），最接近官方 163 key 的 mp3DocId。
    pub md5: Option<String>,
}

/// 单曲可下载性预检结果（来自 /song/detail 的 privilege 字段）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAvailability {
    pub id: u64,
    /// 当前账号是否可以下载该歌曲（dlLevel 非空且 st >= 0）。
    pub downloadable: bool,
    /// 当前账号允许下载的最高音质（privilege.dlLevel），如 lossless / exhigh。
    pub download_level: Option<String>,
    /// 当前账号允许试听的最高音质（privilege.plLevel）。
    pub play_level: Option<String>,
    /// fee：0 免费、1 VIP、4 购买专辑、8 低音质可播。
    pub fee: Option<i64>,
    /// 灰色歌曲（st < 0）。
    pub locked: bool,
    /// 不可下载/不可用的原因码（no_right / gray / region / purchased / none）。
    pub reason: Option<String>,
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

impl ApiCallError {
    pub fn ui(&self) -> crate::error::UiMessage {
        let mut params = Vec::new();
        if let Some(status) = self.meta.http_status {
            params.push(status.to_string());
        }
        if let Some(retry_after) = &self.meta.retry_after {
            params.push(retry_after.clone());
        }
        if let Some(server) = &self.meta.server {
            params.push(server.clone());
        }
        if let Some(request_id) = &self.meta.request_id {
            params.push(request_id.clone());
        }
        crate::error::UiMessage {
            code: self.class.into(),
            params,
        }
    }
}

impl NeteaseApi {
    pub fn download_source(&self) -> &str {
        &self.download_source
    }

    pub fn from_config(config: &Config) -> Result<Self> {
        // 默认（同步引擎/写路径/CLI）：全新空缓存——进程内不跨请求复用，
        // 保证每次读取都基于最新远端数据。
        Self::from_config_with_cache(config, Arc::new(ApiCache::new()))
    }

    /// 构建带共享缓存的 API 客户端。
    /// - UI 展示 / 只读命令：把 AppState 持有的进程级缓存传入，重复读取复用响应。
    /// - 同步引擎 / 写操作：用 `from_config`（全新空缓存），保证每次同步
    ///   都基于最新曲目与权益，绝不被缓存污染。
    pub fn from_config_with_cache(config: &Config, api_cache: Arc<ApiCache>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9"));

        let mut client = Client::builder()
            .default_headers(headers)
            .user_agent(config.ua.clone())
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
            random_cn_ip: config.use_random_cn_ip,
            download_source: config.download_source.clone(),
            api_cache,
        })
    }

    async fn get_value(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> std::result::Result<(Value, Option<String>, ApiResponseMeta), ApiCallError> {
        let url = format!("{}{}", self.base, path);
        let mut query: Vec<(&str, String)> = params.to_vec();
        // 登录/验证码/下载地址路由保持 IP 一致性：随机 IP 会让会员权益判定失效，
        // 服务端会对歌曲地址接口返回试听片段。其余请求可选使用随机中国 IP。
        let stable_ip_path = path.starts_with("/login")
            || path.starts_with("/captcha")
            || path.starts_with("/song/url")
            || path.starts_with("/song/download");
        if self.random_cn_ip && !stable_ip_path {
            query.push(("randomCNIP", "true".into()));
        }
        let mut request = self.client.get(&url).query(&query);
        if let Some(cookie) = &self.cookie {
            // Enhanced 只把 query/body 的 cookie 参数转发给网易上游认证；
            // HTTP Cookie 头不会生效。因此必须同时以 cookie 参数携带会话。
            request = request
                .header("Cookie", cookie)
                .query(&[("cookie", cookie.clone())]);
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
        if self.cookie.is_none() {
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
        }
        let (json, _, meta) = self
            .get_value("/login/status", &[("timestamp", cache_buster())])
            .await
            .map_err(anyhow::Error::new)?;
        let code = json.get("code").and_then(Value::as_i64).unwrap_or(200);
        if code != 200 {
            // 网易云会以 301 表示“需要登录/会话失效”，此时不当作错误，而是明确未登录，
            // 让前端能正确切换登录态。
            if code == 301 {
                return Ok(LoginStatusResponse {
                    status: LoginStatus {
                        logged_in: false,
                        nickname: None,
                        user_id: None,
                        avatar_url: None,
                    },
                    meta,
                    account_present: false,
                });
            }
            return Err(anyhow::Error::new(ApiCallError {
                class: "api_business",
                meta,
            }));
        }
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

    /// 当前用户歌单列表（低频变化，TTL 5 分钟缓存）。
    /// 注意：仅调用方（UI/只读命令）可读缓存；同步引擎用 fresh 实例，不受影响。
    /// 缓存只保存远端元数据；本地配置（启用/覆盖/策略）每次调用都按当前 config 重新合并，
    /// 避免用户改完开关后读到旧值。
    /// `force` 用于 UI“刷新”按钮：穿透缓存直拉网易并回填缓存。
    pub async fn user_playlists(&self, user_id: u64, config: &Config) -> Result<Vec<PlaylistInfo>> {
        self.user_playlists_inner(user_id, config, false).await
    }

    /// 同 [`Self::user_playlists`]，但 `force=true` 时跳过缓存直接请求远端并回填缓存。
    pub async fn user_playlists_forced(
        &self,
        user_id: u64,
        config: &Config,
    ) -> Result<Vec<PlaylistInfo>> {
        self.user_playlists_inner(user_id, config, true).await
    }

    async fn user_playlists_inner(
        &self,
        user_id: u64,
        config: &Config,
        force: bool,
    ) -> Result<Vec<PlaylistInfo>> {
        if !force {
            if let Some(cached) = self
                .api_cache
                .get("user_playlists", &user_id.to_string(), CACHE_TTL_PLAYLIST)
            {
                if let Ok(mut list) = serde_json::from_value::<Vec<PlaylistInfo>>(cached) {
                    merge_playlist_settings(&mut list, config);
                    return Ok(list);
                }
            }
        }
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
        let playlists: Vec<PlaylistInfo> = json
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
                    creator_user_id: playlist
                        .pointer("/creator/userId")
                        .and_then(Value::as_u64),
                    enabled: setting.is_some_and(|setting| setting.enabled),
                    synced: 0,
                    overwrite: setting.is_some_and(|setting| setting.overwrite),
                    last_sync: None,
                    last_result: None,
                    mode_override: setting.and_then(|s| s.mode_override.clone()),
                    upload_manual: setting.and_then(|s| s.upload_manual),
                })
            })
            .collect();
        if let Ok(serialized) = serde_json::to_value(&playlists) {
            self.api_cache
                .put("user_playlists", &user_id.to_string(), serialized);
        }
        Ok(playlists)
    }

    /// 歌单全部曲目（分页拉全）。低频变化，TTL 5 分钟缓存。
    /// 注意：同步引擎、恢复、隔离等“必须基于最新歌单”的路径用 fresh 实例
    /// （每次全新空缓存），天然绕过这里；仅 UI 展示/只读命令共享进程级缓存。
    pub async fn playlist_tracks(&self, id: u64) -> Result<PlaylistTracks> {
        self.playlist_tracks_inner(id, false).await
    }

    /// 同 [`Self::playlist_tracks`]，但 `force=true` 时跳过缓存直拉并回填（UI“刷新”按钮用）。
    pub async fn playlist_tracks_forced(&self, id: u64) -> Result<PlaylistTracks> {
        self.playlist_tracks_inner(id, true).await
    }

    async fn playlist_tracks_inner(&self, id: u64, force: bool) -> Result<PlaylistTracks> {
        if !force {
            if let Some(cached) = self
                .api_cache
                .get("playlist_tracks", &id.to_string(), CACHE_TTL_PLAYLIST)
            {
                if let Ok(playlist) = serde_json::from_value::<PlaylistTracks>(cached) {
                    return Ok(playlist);
                }
            }
        }
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
        if name.is_empty() {
            // /playlist/track/all 不返回歌单名，需从 /playlist/detail 取，否则模板会渲染成“未命名”。
            let (json, _) = self
                .get("/playlist/detail", &[("id", id.to_string())])
                .await?;
            name = json
                .pointer("/playlist/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
        }
        let playlist = PlaylistTracks { id, name, tracks };
        if let Ok(serialized) = serde_json::to_value(&playlist) {
            self.api_cache
                .put("playlist_tracks", &id.to_string(), serialized);
        }
        Ok(playlist)
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
            level: data
                .and_then(|item| item.get("level"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            size: data
                .and_then(|item| item.get("size"))
                .and_then(Value::as_u64),
            br: data
                .and_then(|item| item.get("br"))
                .and_then(Value::as_u64),
            md5: data
                .and_then(|item| item.get("md5"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    /// 歌词文本（对同一首歌基本不变，TTL 24 小时缓存）。
    /// 写 LRC 的同步路径使用 fresh 实例（空缓存），保证每次重写都基于最新歌词。
    pub async fn lyric(&self, id: u64) -> Result<Option<String>> {
        if let Some(cached) = self
            .api_cache
            .get("lyric", &id.to_string(), CACHE_TTL_LYRIC)
        {
            if let Some(text) = cached.as_str() {
                return Ok(Some(text.to_owned()));
            }
            // 缓存了“无歌词”标记（Null）→ 直接返回 None。
            if cached.is_null() {
                return Ok(None);
            }
        }
        let (json, _) = self.get("/lyric", &[("id", id.to_string())]).await?;
        let lyric = json
            .pointer("/lrc/lyric")
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.api_cache
            .put("lyric", &id.to_string(), lyric.clone().map_or(Value::Null, Value::String));
        Ok(lyric)
    }

    /// 批量获取歌曲详情（支持逗号分隔多个 id，一次最多 ~100 个）。
    /// 返回 id -> detail(含 privilege 音质/版权信息)。
    pub async fn song_detail_batch(&self, ids: &[u64]) -> Result<HashMap<u64, serde_json::Value>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let joined = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
        let (json, _) = self.get("/song/detail", &[("ids", joined)]).await?;
        let mut out = HashMap::new();
        if let Some(songs) = json.get("songs").and_then(Value::as_array) {
            for song in songs {
                if let Some(id) = song.get("id").and_then(Value::as_u64) {
                    out.insert(id, song.clone());
                }
            }
        }
        Ok(out)
    }

    /// 基于 /song/detail 的 privilege 信息做可下载性/最高音质预检（不依赖付费探测）。
    pub async fn preflight_tracks(&self, tracks: &[Track]) -> Result<Vec<TrackAvailability>> {
        if tracks.is_empty() {
            return Ok(vec![]);
        }
        let ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
        let details = self.song_detail_batch(&ids).await?;
        let mut out = Vec::with_capacity(ids.len());
        for track in tracks {
            let Some(detail) = details.get(&track.id) else {
                out.push(TrackAvailability {
                    id: track.id,
                    downloadable: true,
                    download_level: None,
                    play_level: None,
                    fee: None,
                    locked: false,
                    reason: None,
                });
                continue;
            };
            let fee = detail.get("fee").and_then(Value::as_i64);
            let privilege = detail.get("privilege");
            let st = privilege
                .and_then(|p| p.get("st"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let locked = st < 0;
            let download_level = privilege
                .and_then(|p| p.get("dlLevel"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .filter(|s| !s.is_empty());
            let play_level = privilege
                .and_then(|p| p.get("plLevel"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .filter(|s| !s.is_empty());
            let toast = privilege
                .and_then(|p| p.get("toast"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let reason = if locked {
                if toast {
                    Some("region".into())
                } else {
                    Some("gray".into())
                }
            } else if fee == Some(4) && download_level.is_none() {
                Some("purchased".into())
            } else if download_level.is_none() && fee == Some(1) {
                Some("no_right".into())
            } else {
                None
            };
            out.push(TrackAvailability {
                id: track.id,
                downloadable: !locked && !toast && (fee != Some(4) || download_level.is_some()),
                download_level,
                play_level,
                fee,
                locked,
                reason,
            });
        }
        Ok(out)
    }

    /// 批量获取直链（/song/url/v1 一次多 id）。服务端对“个别 id 无版权”会整组
    /// 返回空 url，因此拿不到地址的 id 再按音质链逐个请求兜底。
    pub async fn song_url_batch(
        &self,
        ids: &[u64],
        quality: &str,
    ) -> Result<HashMap<u64, SongUrl>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut result = HashMap::new();
        // 每批最多 60 个 id，避免 URL 过长。
        for chunk in ids.chunks(60) {
            let joined = chunk
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let (json, _) = self
                .get(
                    "/song/url/v1",
                    &[("id", joined), ("level", quality.to_owned())],
                )
                .await?;
            if let Some(data) = json.get("data").and_then(Value::as_array) {
                for item in data {
                    let Some(id) = item.get("id").and_then(Value::as_u64) else {
                        continue;
                    };
                    let url = item.get("url").and_then(Value::as_str).map(str::to_owned);
                    if let Some(url) = url {
                        result.insert(
                            id,
                            SongUrl {
                                url: Some(url.replace("http://", "https://")),
                                file_type: item
                                    .get("type")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned),
                                level: item.get("level").and_then(Value::as_str).map(str::to_owned),
                                size: item.get("size").and_then(Value::as_u64),
                                br: item.get("br").and_then(Value::as_u64),
                                md5: item.get("md5").and_then(Value::as_str).map(str::to_owned),
                            },
                        );
                    }
                }
            }
        }
        // 兜底：批量整体失败 / 空地址的 id，按音质链逐个请求。
        let attempts: Vec<String> = quality_fallback_chain(quality)
            .iter()
            .map(|level| (*level).to_owned())
            .collect();
        for id in ids {
            if result.contains_key(id) {
                continue;
            }
            for level in &attempts {
                if let Ok(song_url) = self.song_url(*id, level).await {
                    if let Some(url) = song_url.url.clone() {
                        result.insert(
                            *id,
                            SongUrl {
                                url: Some(url.replace("http://", "https://")),
                                file_type: song_url.file_type,
                                level: song_url.level,
                                size: song_url.size,
                                br: song_url.br,
                                md5: song_url.md5,
                            },
                        );
                        break;
                    }
                }
            }
            if !result.contains_key(id) {
                if let Some((url, format)) = self.song_download_url_candidate(*id).await {
                    result.insert(
                        *id,
                        SongUrl {
                            url: Some(url.replace("http://", "https://")),
                            file_type: Some(format),
                            level: None,
                            size: None,
                            br: None,
                            md5: None,
                        },
                    );
                }
            }
        }
        Ok(result)
    }

    /// 当前账号“我喜欢”的歌曲 id 列表（低频变化，TTL 10 分钟缓存）。
    /// 同步/写路径用 fresh 实例；备份/统计等只读路径复用缓存减少重复请求。
    pub async fn liked_song_ids(&self, user_id: u64) -> Result<Vec<u64>> {
        self.liked_song_ids_inner(user_id, false).await
    }

    /// 同 [`Self::liked_song_ids`]，但 `force=true` 时跳过缓存直拉并回填。
    pub async fn liked_song_ids_forced(&self, user_id: u64) -> Result<Vec<u64>> {
        self.liked_song_ids_inner(user_id, true).await
    }

    async fn liked_song_ids_inner(&self, user_id: u64, force: bool) -> Result<Vec<u64>> {
        if !force {
            if let Some(cached) = self
                .api_cache
                .get("likelist", &user_id.to_string(), CACHE_TTL_ACCOUNT)
            {
                if let Ok(ids) = serde_json::from_value::<Vec<u64>>(cached) {
                    return Ok(ids);
                }
            }
        }
        let (json, _) = self
            .get("/likelist", &[("uid", user_id.to_string())])
            .await?;
        let ids: Vec<u64> = json
            .get("ids")
            .and_then(Value::as_array)
            .unwrap_or(&vec![])
            .iter()
            .filter_map(Value::as_u64)
            .collect();
        if let Ok(serialized) = serde_json::to_value(&ids) {
            self.api_cache
                .put("likelist", &user_id.to_string(), serialized);
        }
        Ok(ids)
    }

    /// 获取已购买/已下载（单曲）歌曲列表（分页拉全）。
    pub async fn purchased_songs(&self) -> Result<Vec<u64>> {
        let mut ids = Vec::new();
        let mut offset = 0i64;
        loop {
            let (json, _) = self
                .get(
                    "/song/purchased",
                    &[("limit", "200".into()), ("offset", offset.to_string())],
                )
                .await?;
            let page: Vec<u64> = json
                .get("data")
                .and_then(Value::as_array)
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|item| {
                    item.get("id").and_then(Value::as_u64).or_else(|| {
                        item.get("song")
                            .and_then(|s| s.get("id"))
                            .and_then(Value::as_u64)
                    })
                })
                .collect();
            let count = page.len();
            ids.extend(page);
            if count < 200 {
                break;
            }
            offset += count as i64;
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    /// 发送手机验证码。
    pub async fn send_captcha(&self, phone: &str, ctcode: &str) -> Result<()> {
        let (json, _) = self
            .get(
                "/captcha/sent",
                &[
                    ("phone", phone.to_owned()),
                    ("ctcode", ctcode.to_owned()),
                    ("timestamp", cache_buster()),
                ],
            )
            .await?;
        ensure_success_code(&json)?;
        Ok(())
    }

    /// 校验验证码（可选步骤，用于把“发送验证码”到“验证码登录”串起来）。
    pub async fn verify_captcha(&self, phone: &str, captcha: &str) -> Result<bool> {
        let (json, _) = self
            .get(
                "/captcha/verify",
                &[
                    ("phone", phone.to_owned()),
                    ("captcha", captcha.to_owned()),
                    ("ctcode", "86".into()),
                    ("timestamp", cache_buster()),
                ],
            )
            .await?;
        Ok(json.get("data").and_then(Value::as_bool).unwrap_or(false))
    }

    /// 验证码登录（优先于密码，避免密码风险）。
    pub async fn login_cellphone(
        &self,
        phone: &str,
        captcha: &str,
    ) -> Result<(String, Option<LoginStatus>)> {
        let (json, set_cookie, meta) = self
            .get_value(
                "/login/cellphone",
                &[
                    ("phone", phone.to_owned()),
                    ("captcha", captcha.to_owned()),
                    ("ctcode", "86".into()),
                    ("timestamp", cache_buster()),
                ],
            )
            .await
            .map_err(anyhow::Error::new)?;
        let code = json.get("code").and_then(Value::as_i64).unwrap_or(200);
        if code != 200 {
            return Err(anyhow::Error::new(ApiCallError {
                class: "api_business",
                meta,
            }));
        }
        let cookie = json
            .get("cookie")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or(set_cookie)
            .ok_or_else(|| anyhow!(UiMessage::new("cookie_missing")))?;
        let profile = json.get("profile");
        let status = LoginStatus {
            logged_in: true,
            nickname: profile
                .and_then(|p| p.get("nickname"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            user_id: profile
                .and_then(|p| p.get("userId"))
                .and_then(Value::as_u64),
            avatar_url: profile
                .and_then(|p| p.get("avatarUrl"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        };
        Ok((cookie, Some(status)))
    }

    /// 判断歌单是否本人创建：/playlist/detail 的 creator.userId 与当前登录 uid 比对。
    pub async fn playlist_owned_by_me(&self, playlist_id: u64, my_uid: u64) -> Result<bool> {
        let (json, _) = self
            .get("/playlist/detail", &[("id", playlist_id.to_string())])
            .await?;
        let creator_uid = json
            .pointer("/playlist/creator/userId")
            .and_then(Value::as_u64);
        Ok(creator_uid == Some(my_uid))
    }

    /// 往歌单加入曲目（需要登录；仅对“我创建的”歌单有效）。
    /// 批量分批（每批 50），返回实际加入数量。
    pub async fn playlist_add_tracks(&self, pid: u64, ids: &[u64]) -> Result<usize> {
        self.playlist_mutate("add", pid, ids).await
    }

    /// 从歌单移除曲目（需要登录；仅对“我创建的”歌单有效）。
    pub async fn playlist_remove_tracks(&self, pid: u64, ids: &[u64]) -> Result<usize> {
        self.playlist_mutate("del", pid, ids).await
    }

    async fn playlist_mutate(&self, op: &str, pid: u64, ids: &[u64]) -> Result<usize> {
        let mut done = 0usize;
        for chunk in ids.chunks(50) {
            let joined = chunk
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let (json, _) = self
                .get(
                    "/playlist/tracks",
                    &[
                        ("op", op.into()),
                        ("pid", pid.to_string()),
                        ("tracks", joined),
                        ("timestamp", cache_buster()),
                    ],
                )
                .await?;
            let code = json.get("code").and_then(Value::as_i64).unwrap_or(200);
            if code != 200 {
                let message = json
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("网易云 API 返回错误")
                    .to_owned();
                anyhow::bail!("playlist {} failed: {code} {message}", op);
            }
            done += chunk.len();
        }
        Ok(done)
    }

    /// 用本地文件信息匹配网易曲目（/search/match）。
    /// 返回匹配到的 song id（取第一个结果的 id）。
    pub async fn search_match_local(
        &self,
        title: &str,
        album: &str,
        artist: &str,
        duration_secs: f64,
        md5: &str,
    ) -> Result<Option<u64>> {
        let (json, _) = self
            .get(
                "/search/match",
                &[
                    ("title", title.to_owned()),
                    ("album", album.to_owned()),
                    ("artist", artist.to_owned()),
                    ("duration", format!("{:.2}", duration_secs)),
                    ("md5", md5.to_owned()),
                    ("timestamp", cache_buster()),
                ],
            )
            .await?;
        let code = json.get("code").and_then(Value::as_i64).unwrap_or(200);
        if code != 200 {
            return Ok(None);
        }
        let songs = json
            .get("result")
            .and_then(|r| r.get("songs"))
            .and_then(Value::as_array);
        let first = songs.and_then(|arr| arr.first());
        Ok(first
            .and_then(|song| song.get("id"))
            .and_then(Value::as_u64))
    }

    /// 用户详情（/user/detail）→ 返回 profile 顶层（level 等在 data 内）。
    /// 账号资料低频变化，TTL 10 分钟缓存。
    pub async fn user_detail(&self, uid: u64) -> Result<serde_json::Value> {
        self.user_detail_inner(uid, false).await
    }

    /// 同 [`Self::user_detail`]，但 `force=true` 时跳过缓存直拉并回填。
    pub async fn user_detail_forced(&self, uid: u64) -> Result<serde_json::Value> {
        self.user_detail_inner(uid, true).await
    }

    async fn user_detail_inner(&self, uid: u64, force: bool) -> Result<serde_json::Value> {
        if !force {
            if let Some(cached) = self
                .api_cache
                .get("user_detail", &uid.to_string(), CACHE_TTL_ACCOUNT)
            {
                return Ok(cached);
            }
        }
        let (json, _) = self
            .get("/user/detail", &[("uid", uid.to_string())])
            .await?;
        let profile = json
            .get("profile")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        self.api_cache.put("user_detail", &uid.to_string(), profile.clone());
        Ok(profile)
    }

    /// 用户计数（/user/subcount）：歌单/收藏/mv/dj 数量。
    /// 实测该接口把字段放在 JSON 顶层（无 data 包裹），故返回整包由调用方取键。
    /// 低频变化，TTL 10 分钟缓存。
    pub async fn user_subcount(&self) -> Result<serde_json::Value> {
        self.user_subcount_inner(false).await
    }

    /// 同 [`Self::user_subcount`]，但跳过缓存直拉并回填。
    pub async fn user_subcount_forced(&self) -> Result<serde_json::Value> {
        self.user_subcount_inner(true).await
    }

    async fn user_subcount_inner(&self, force: bool) -> Result<serde_json::Value> {
        if !force {
            if let Some(cached) = self.api_cache.get("user_subcount", "self", CACHE_TTL_ACCOUNT) {
                return Ok(cached);
            }
        }
        let (json, _) = self.get("/user/subcount", &[]).await?;
        self.api_cache
            .put("user_subcount", "self", json.clone());
        Ok(json)
    }

    /// 用户等级（/user/level）。低频变化，TTL 10 分钟缓存。
    pub async fn user_level(&self) -> Result<serde_json::Value> {
        self.user_level_inner(false).await
    }

    /// 同 [`Self::user_level`]，但跳过缓存直拉并回填。
    pub async fn user_level_forced(&self) -> Result<serde_json::Value> {
        self.user_level_inner(true).await
    }

    async fn user_level_inner(&self, force: bool) -> Result<serde_json::Value> {
        if !force {
            if let Some(cached) = self.api_cache.get("user_level", "self", CACHE_TTL_ACCOUNT) {
                return Ok(cached);
            }
        }
        let (json, _) = self.get("/user/level", &[]).await?;
        let level = json
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        self.api_cache.put("user_level", "self", level.clone());
        Ok(level)
    }

    /// VIP 信息：优先 /vip/info/v2，失败回 /vip/info。
    /// VIP 权益变动会影响预检（那是独立路径，不缓存）；此处仅账号页展示，TTL 10 分钟。
    pub async fn vip_info(&self) -> Result<serde_json::Value> {
        self.vip_info_inner(false).await
    }

    /// 同 [`Self::vip_info`]，但跳过缓存直拉并回填。
    pub async fn vip_info_forced(&self) -> Result<serde_json::Value> {
        self.vip_info_inner(true).await
    }

    async fn vip_info_inner(&self, force: bool) -> Result<serde_json::Value> {
        if !force {
            if let Some(cached) = self.api_cache.get("vip_info", "self", CACHE_TTL_ACCOUNT) {
                return Ok(cached);
            }
        }
        let v2 = self.get("/vip/info/v2", &[]).await;
        let data = if let Ok((json, _)) = v2 {
            if json.get("code").and_then(Value::as_i64).unwrap_or(200) == 200 {
                json.get("data")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null)
            } else {
                let (json, _) = self.get("/vip/info", &[]).await?;
                json.get("data").cloned().unwrap_or(serde_json::Value::Null)
            }
        } else {
            let (json, _) = self.get("/vip/info", &[]).await?;
            json.get("data").cloned().unwrap_or(serde_json::Value::Null)
        };
        self.api_cache.put("vip_info", "self", data.clone());
        Ok(data)
    }

    /// 当 /song/url/v1 拿不到地址时的兜底候选：依次尝试
    /// /song/download/url/v1 与 /song/download/url?br=320000。
    pub async fn song_download_url_candidate(&self, id: u64) -> Option<(String, String)> {
        let attempts: [(&str, Vec<(&str, String)>); 2] = [
            (
                "/song/download/url/v1",
                vec![("id", id.to_string()), ("level", "exhigh".into())],
            ),
            (
                "/song/download/url",
                vec![("id", id.to_string()), ("br", "320000".into())],
            ),
        ];
        for (path, params) in attempts {
            if let Ok((json, _)) = self.get(path, &params).await {
                if let Some(candidate) = parse_download_url(&json) {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

pub(crate) fn quality_fallback_chain(quality: &str) -> &'static [&'static str] {
    match quality {
        "hires" => &["hires", "lossless", "exhigh", "higher", "standard"],
        "lossless" => &["lossless", "exhigh", "higher", "standard"],
        "exhigh" => &["exhigh", "higher", "standard"],
        "higher" => &["higher", "standard"],
        _ => &["standard"],
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

fn parse_download_url(json: &Value) -> Option<(String, String)> {    let data = json.get("data")?;
    if let Some(array) = data.as_array() {
        let first = array.first()?;
        let url = first.get("url")?.as_str()?;
        let format = first.get("type").and_then(Value::as_str).unwrap_or("mp3");
        return Some((url.to_owned(), format.to_owned()));
    }
    if let Some(url) = data.as_str() {
        return Some((url.to_owned(), "mp3".into()));
    }
    let url = data.get("url")?.as_str()?;
    Some((url.to_owned(), "mp3".into()))
}

/// 把本地配置（启用/覆盖/策略）重新合并进歌单元数据。
/// 远端元数据可缓存，但这些字段来自 config，变化时需即时反映。
fn merge_playlist_settings(playlists: &mut [PlaylistInfo], config: &Config) {
    for playlist in playlists.iter_mut() {
        let Some(setting) = config.playlists.iter().find(|s| s.id == playlist.id) else {
            continue;
        };
        playlist.enabled = setting.enabled;
        playlist.overwrite = setting.overwrite;
        playlist.mode_override.clone_from(&setting.mode_override);
        playlist.upload_manual = setting.upload_manual;
    }
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
