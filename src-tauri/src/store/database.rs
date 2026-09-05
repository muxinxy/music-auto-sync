use anyhow::Result;
use chrono::Local;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineItem {
    pub id: i64,
    pub playlist_name: String,
    pub file_name: String,
    pub original_path: String,
    pub quarantine_path: String,
    pub quarantined_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncLogEntry {
    pub id: i64,
    pub ts: String,
    pub playlist_name: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncChangeEntry {
    pub id: i64,
    pub sync_run_id: i64,
    pub ts: String,
    pub playlist_id: u64,
    pub playlist_name: String,
    pub direction: String,
    pub action: String,
    pub track_id: Option<u64>,
    pub track_name: Option<String>,
    pub local_path: Option<String>,
    pub quarantined_path: Option<String>,
    pub netease_id: Option<u64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistHistoryEntry {
    pub id: i64,
    pub playlist_id: u64,
    pub ts: String,
    pub playlist_name: String,
    pub snapshot: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedLogEntry {
    pub id: i64,
    pub ts: String,
    pub kind: String,
    pub playlist_id: u64,
    pub playlist_name: String,
    pub track_id: Option<u64>,
    pub track_name: Option<String>,
    pub local_path: Option<String>,
    pub quarantined_path: Option<String>,
    pub netease_id: Option<u64>,
    pub restored_at: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalStats {
    pub total_sync_runs: u64,
    pub total_added: u64,
    pub total_quarantined: u64,
    pub total_ncm_converted: u64,
    pub total_failed: u64,
    pub current_local_files: u64,
    pub quarantine_items: u64,
    pub history_snapshots: u64,
}

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "
        PRAGMA journal_mode=WAL;
        PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS track_files (
            playlist_id INTEGER NOT NULL,
            track_id INTEGER NOT NULL,
            local_path TEXT NOT NULL,
            source_format TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (playlist_id, track_id)
        );
        CREATE TABLE IF NOT EXISTS playlist_snapshots (
            playlist_id INTEGER NOT NULL,
            track_id INTEGER NOT NULL,
            position INTEGER NOT NULL,
            snapshot_at TEXT NOT NULL,
            PRIMARY KEY (playlist_id, track_id)
        );
        CREATE TABLE IF NOT EXISTS quarantine (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            playlist_id INTEGER NOT NULL,
            playlist_name TEXT NOT NULL,
            file_name TEXT NOT NULL,
            original_path TEXT NOT NULL,
            quarantine_path TEXT NOT NULL UNIQUE,
            quarantined_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sync_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            playlist_name TEXT NOT NULL,
            status TEXT NOT NULL,
            message TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sync_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            playlist_id INTEGER NOT NULL,
            playlist_name TEXT NOT NULL,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            added INTEGER NOT NULL DEFAULT 0,
            updated INTEGER NOT NULL DEFAULT 0,
            quarantined INTEGER NOT NULL DEFAULT 0,
            ncm_converted INTEGER NOT NULL DEFAULT 0,
            failed INTEGER NOT NULL DEFAULT 0,
            skipped INTEGER NOT NULL DEFAULT 0,
            errors TEXT NOT NULL DEFAULT '[]'
        );
        -- 同步变更流水：每次同步的每条新增/删除都记录，供查看与恢复。
        CREATE TABLE IF NOT EXISTS sync_changes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sync_run_id INTEGER NOT NULL DEFAULT 0,
            ts TEXT NOT NULL,
            playlist_id INTEGER NOT NULL,
            playlist_name TEXT NOT NULL,
            direction TEXT NOT NULL,      -- to_local | to_playlist
            action TEXT NOT NULL,          -- added_local | quarantined_local | added_playlist | removed_from_playlist | failed
            track_id INTEGER,
            track_name TEXT,
            local_path TEXT,
            quarantined_path TEXT,
            netease_id INTEGER,
            note TEXT
        );
        -- 歌单歌曲快照历史：每次同步后保存全量 id 列表，支持恢复到某个历史状态。
        CREATE TABLE IF NOT EXISTS playlist_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            playlist_id INTEGER NOT NULL,
            ts TEXT NOT NULL,
            playlist_name TEXT NOT NULL,
            snapshot TEXT NOT NULL,        -- JSON 数组：[{id,name,position}]
            source TEXT NOT NULL           -- auto | manual | scheduled | tray | cli
        );
        -- 删除日志：网易歌单被移出的曲目 / 本地被隔离的文件，供恢复。
        CREATE TABLE IF NOT EXISTS deleted_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            kind TEXT NOT NULL,            -- local_file | playlist_track
            playlist_id INTEGER NOT NULL,
            playlist_name TEXT NOT NULL,
            track_id INTEGER,
            track_name TEXT,
            local_path TEXT,
            quarantined_path TEXT,
            netease_id INTEGER,
            restored_at TEXT,
            note TEXT
        );
        ",
    )?;
    Ok(conn)
}

pub fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn record_sync_run(conn: &Connection, report: &crate::core::sync::SyncReport) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_runs(playlist_id,playlist_name,started_at,finished_at,added,updated,quarantined,ncm_converted,failed,skipped,errors)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            report.playlist_id as i64,
            report.playlist_name,
            report.started_at,
            report.finished_at,
            report.added as i64,
            report.updated as i64,
            report.quarantined as i64,
            report.ncm_converted as i64,
            report.failed as i64,
            report.skipped as i64,
            serde_json::to_string(&report.errors).unwrap_or_else(|_| "[]".into()),
        ],
    )?;
    Ok(())
}

pub fn log(conn: &Connection, playlist_name: &str, status: &str, message: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO sync_logs(ts, playlist_name, status, message) VALUES (?1, ?2, ?3, ?4)",
        params![now(), playlist_name, status, message],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_logs(conn: &Connection, limit: usize) -> Result<Vec<SyncLogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, playlist_name, status, message FROM sync_logs ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| {
        Ok(SyncLogEntry {
            id: row.get(0)?,
            ts: row.get(1)?,
            playlist_name: row.get(2)?,
            status: row.get(3)?,
            message: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn add_quarantine(
    conn: &Connection,
    playlist_id: u64,
    playlist_name: &str,
    file_name: &str,
    original_path: &str,
    quarantine_path: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO quarantine(playlist_id, playlist_name, file_name, original_path, quarantine_path, quarantined_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![playlist_id, playlist_name, file_name, original_path, quarantine_path, now()],
    )?;
    Ok(())
}

pub fn list_quarantine(conn: &Connection) -> Result<Vec<QuarantineItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, playlist_name, file_name, original_path, quarantine_path, quarantined_at FROM quarantine ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(QuarantineItem {
            id: row.get(0)?,
            playlist_name: row.get(1)?,
            file_name: row.get(2)?,
            original_path: row.get(3)?,
            quarantine_path: row.get(4)?,
            quarantined_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

// ---------- 变更流水 / 快照 / 删除日志 ----------

#[allow(clippy::too_many_arguments)]
pub fn record_change(
    conn: &Connection,
    sync_run_id: i64,
    playlist_id: u64,
    playlist_name: &str,
    direction: &str,
    action: &str,
    track_id: Option<u64>,
    track_name: Option<&str>,
    local_path: Option<&str>,
    quarantined_path: Option<&str>,
    netease_id: Option<u64>,
    note: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO sync_changes(sync_run_id, ts, playlist_id, playlist_name, direction, action, track_id, track_name, local_path, quarantined_path, netease_id, note)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            sync_run_id,
            now(),
            playlist_id as i64,
            playlist_name,
            direction,
            action,
            track_id.map(|v| v as i64),
            track_name,
            local_path,
            quarantined_path,
            netease_id.map(|v| v as i64),
            note
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_changes(conn: &Connection, limit: usize) -> Result<Vec<SyncChangeEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, sync_run_id, ts, playlist_id, playlist_name, direction, action, track_id, track_name, local_path, quarantined_path, netease_id, note
         FROM sync_changes ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |row| {
        Ok(SyncChangeEntry {
            id: row.get(0)?,
            sync_run_id: row.get(1)?,
            ts: row.get(2)?,
            playlist_id: row.get::<_, i64>(3)? as u64,
            playlist_name: row.get(4)?,
            direction: row.get(5)?,
            action: row.get(6)?,
            track_id: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
            track_name: row.get(8)?,
            local_path: row.get(9)?,
            quarantined_path: row.get(10)?,
            netease_id: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
            note: row.get(12)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn record_playlist_snapshot(
    conn: &Connection,
    playlist_id: u64,
    playlist_name: &str,
    snapshot: &str,
    source: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO playlist_history(playlist_id, ts, playlist_name, snapshot, source) VALUES(?1,?2,?3,?4,?5)",
        params![playlist_id as i64, now(), playlist_name, snapshot, source],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_playlist_history(
    conn: &Connection,
    playlist_id: u64,
    limit: usize,
) -> Result<Vec<PlaylistHistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, playlist_id, ts, playlist_name, snapshot, source FROM playlist_history
         WHERE playlist_id=?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![playlist_id as i64, limit as i64], |row| {
            Ok(PlaylistHistoryEntry {
                id: row.get(0)?,
                playlist_id: row.get::<_, i64>(1)? as u64,
                ts: row.get(2)?,
                playlist_name: row.get(3)?,
                snapshot: row.get(4)?,
                source: row.get(5)?,
            })
        })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get_playlist_history(
    conn: &Connection,
    history_id: i64,
) -> Result<Option<PlaylistHistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, playlist_id, ts, playlist_name, snapshot, source FROM playlist_history WHERE id=?1",
    )?;
    let mut rows = stmt.query_map([history_id], |row| {
        Ok(PlaylistHistoryEntry {
            id: row.get(0)?,
            playlist_id: row.get::<_, i64>(1)? as u64,
            ts: row.get(2)?,
            playlist_name: row.get(3)?,
            snapshot: row.get(4)?,
            source: row.get(5)?,
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn record_deleted(
    conn: &Connection,
    kind: &str,
    playlist_id: u64,
    playlist_name: &str,
    track_id: Option<u64>,
    track_name: Option<&str>,
    local_path: Option<&str>,
    quarantined_path: Option<&str>,
    netease_id: Option<u64>,
    note: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO deleted_log(ts, kind, playlist_id, playlist_name, track_id, track_name, local_path, quarantined_path, netease_id, note)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            now(),
            kind,
            playlist_id as i64,
            playlist_name,
            track_id.map(|v| v as i64),
            track_name,
            local_path,
            quarantined_path,
            netease_id.map(|v| v as i64),
            note
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_deleted(conn: &Connection, limit: usize) -> Result<Vec<DeletedLogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, kind, playlist_id, playlist_name, track_id, track_name, local_path, quarantined_path, netease_id, restored_at, note
         FROM deleted_log ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |row| {
        Ok(DeletedLogEntry {
            id: row.get(0)?,
            ts: row.get(1)?,
            kind: row.get(2)?,
            playlist_id: row.get::<_, i64>(3)? as u64,
            playlist_name: row.get(4)?,
            track_id: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
            track_name: row.get(6)?,
            local_path: row.get(7)?,
            quarantined_path: row.get(8)?,
            netease_id: row.get::<_, Option<i64>>(9)?.map(|v| v as u64),
            restored_at: row.get(10)?,
            note: row.get(11)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn mark_deleted_restored(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE deleted_log SET restored_at=?1 WHERE id=?2",
        params![now(), id],
    )?;
    Ok(())
}

/// 汇总本地同步统计（账号页展示）。
pub fn summarize_stats(conn: &Connection) -> Result<LocalStats> {    let mut stats = LocalStats::default();
    let sum_col = |conn: &Connection, sql: &str| -> u64 {
        conn.query_row(sql, [], |row| row.get::<_, Option<i64>>(0))
            .ok()
            .flatten()
            .unwrap_or(0) as u64
    };
    stats.total_sync_runs = sum_col(conn, "SELECT COUNT(*) FROM sync_runs");
    stats.total_added = sum_col(conn, "SELECT SUM(added) FROM sync_runs");
    stats.total_quarantined = sum_col(conn, "SELECT SUM(quarantined) FROM sync_runs");
    stats.total_ncm_converted = sum_col(conn, "SELECT SUM(ncm_converted) FROM sync_runs");
    stats.total_failed = sum_col(conn, "SELECT SUM(failed) FROM sync_runs");
    stats.current_local_files = sum_col(conn, "SELECT COUNT(*) FROM track_files");
    stats.quarantine_items = sum_col(conn, "SELECT COUNT(*) FROM quarantine");
    stats.history_snapshots = sum_col(conn, "SELECT COUNT(*) FROM playlist_history");
    Ok(stats)
}

/// 清空指定类型的历史记录（不触碰隔离文件本身）。
/// kind: logs=同步日志, changes=变更流水, deleted=删除记录, history=歌单快照。
pub fn clear_history(conn: &Connection, kind: &str) -> Result<usize> {
    let table = match kind {
        "logs" => "sync_logs",
        "changes" => "sync_changes",
        "deleted" => "deleted_log",
        "history" => "playlist_history",
        _ => return Err(anyhow::anyhow!("unknown history kind: {kind}")),
    };
    let sql = format!("DELETE FROM {table}");
    let count = conn.execute(&sql, [])?;
    Ok(count)
}
