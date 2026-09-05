//! 进程内 TTL 响应缓存。
//!
//! 只缓存“低频变化、时效不敏感”的网易元数据（歌单列表/曲目/歌词/账号资料），
//! 供 UI 展示与只读命令复用，避免重复网络请求。易变数据（登录态、下载直链、
//! VIP 权益预检）与同步引擎路径一律不走这里——见 `NeteaseApi` 内注释。
//!
//! 生命周期：随 AppState 持有；登出/登录、切换数据目录、apiBase/cookie 变化时
//! `clear_all` 全清。无需落盘（重启即失效，天然无脏数据）。

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 单条缓存条目：序列化后的响应体 + 写入时刻（用于 TTL 判定）。
struct CacheEntry {
    value: Value,
    stored_at: Instant,
}

/// 缓存键：命名空间 + 业务键（user_id / 歌单 id / 曲目 id 等）。
pub type CacheKey = (String, String);

/// 容量上限：防止极端场景（大量歌单/曲目）撑爆内存。超出时清理最旧条目。
const MAX_ENTRIES: usize = 2048;

pub struct ApiCache {
    inner: Mutex<HashMap<CacheKey, CacheEntry>>,
}

impl ApiCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 返回命中的缓存值；条目缺失或已超过 `ttl` 均视为 miss（调用方回源并重新写入）。
    pub fn get(&self, namespace: &str, key: &str, ttl: Duration) -> Option<Value> {
        let mut guard = self.inner.lock().ok()?;
        let entry = guard.get(&(namespace.to_owned(), key.to_owned()))?;
        if entry.stored_at.elapsed() > ttl {
            guard.remove(&(namespace.to_owned(), key.to_owned()));
            return None;
        }
        Some(entry.value.clone())
    }

    /// 写入缓存条目（覆盖旧值）。
    pub fn put(&self, namespace: &str, key: &str, value: Value) {
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        let entry_key = (namespace.to_owned(), key.to_owned());
        let is_new = !guard.contains_key(&entry_key);
        guard.insert(
            entry_key,
            CacheEntry {
                value,
                stored_at: Instant::now(),
            },
        );
        if is_new && guard.len() > MAX_ENTRIES {
            evict_oldest(&mut guard);
        }
    }

    /// 清理某个命名空间下的所有条目（例如某歌单曲目已变更）。
    pub fn invalidate_namespace(&self, namespace: &str) {
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        guard.retain(|(ns, _), _| ns != namespace);
    }

    /// 全清（登出/登录、数据目录切换、apiBase/cookie 变化时调用）。
    pub fn clear_all(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.clear();
        }
    }
}

impl Default for ApiCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 超限时移除最旧的少量条目，避免频繁全量扫描。
fn evict_oldest(map: &mut HashMap<CacheKey, CacheEntry>) {
    let mut oldest: Vec<(CacheKey, Instant)> = map
        .iter()
        .map(|(key, entry)| (key.clone(), entry.stored_at))
        .collect();
    oldest.sort_by_key(|(_, at)| *at);
    // 清理约 10% 最旧条目，留出余量。
    let remove_count = (oldest.len() / 10).max(1);
    for (key, _) in oldest.into_iter().take(remove_count) {
        map.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serves_within_ttl_and_expires_after() {
        let cache = ApiCache::new();
        cache.put("user_playlists", "u1", json!([1, 2, 3]));
        // TTL 未到 → 命中。
        assert_eq!(
            cache.get("user_playlists", "u1", Duration::from_secs(60)),
            Some(json!([1, 2, 3]))
        );
        // TTL 已过 → miss。
        assert_eq!(cache.get("user_playlists", "u1", Duration::ZERO), None);
    }

    #[test]
    fn namespace_isolates_keys() {
        let cache = ApiCache::new();
        cache.put("playlist_tracks", "100", json!({"a": 1}));
        cache.put("playlist_tracks", "200", json!({"b": 2}));
        cache.put("lyric", "100", json!("词"));
        cache.invalidate_namespace("playlist_tracks");
        assert!(cache.get("playlist_tracks", "100", Duration::from_secs(60)).is_none());
        assert!(cache.get("playlist_tracks", "200", Duration::from_secs(60)).is_none());
        // 其他命名空间不受影响。
        assert_eq!(cache.get("lyric", "100", Duration::from_secs(60)), Some(json!("词")));
    }

    #[test]
    fn clear_all_empties_everything() {
        let cache = ApiCache::new();
        cache.put("a", "1", json!(1));
        cache.put("b", "2", json!(2));
        cache.clear_all();
        assert!(cache.get("a", "1", Duration::from_secs(60)).is_none());
        assert!(cache.get("b", "2", Duration::from_secs(60)).is_none());
    }

    #[test]
    fn put_overwrites_existing_value() {
        let cache = ApiCache::new();
        cache.put("k", "v", json!("old"));
        cache.put("k", "v", json!("new"));
        assert_eq!(
            cache.get("k", "v", Duration::from_secs(60)),
            Some(json!("new"))
        );
    }
}
