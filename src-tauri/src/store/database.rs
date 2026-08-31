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
        "
    )?;
    Ok(conn)
}

pub fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn log(conn: &Connection, playlist_name: &str, status: &str, message: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_logs(ts, playlist_name, status, message) VALUES (?1, ?2, ?3, ?4)",
        params![now(), playlist_name, status, message],
    )?;
    Ok(())
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
