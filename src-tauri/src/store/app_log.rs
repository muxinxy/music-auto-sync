use anyhow::Result;
use chrono::Local;
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FILES: usize = 3;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLogEntry {
    pub ts: String,
    pub level: &'static str,
    pub module: String,
    pub message: String,
}

pub fn log(
    logs_dir: &Path,
    level: &'static str,
    module: &str,
    message: impl Into<String>,
) -> Result<()> {
    let entry = AppLogEntry {
        ts: Local::now().to_rfc3339(),
        level,
        module: module.to_owned(),
        message: message.into(),
    };
    fs::create_dir_all(logs_dir)?;
    let path = logs_dir.join("app.log.jsonl");
    // 滚动：超过阈值时把 app.log.jsonl.2 -> 删除, .1 -> .2, 当前 -> .1
    if path.is_file() && fs::metadata(&path)?.len() >= MAX_BYTES {
        let older = logs_dir.join("app.log.jsonl.2");
        let old = logs_dir.join("app.log.jsonl.1");
        if older.exists() {
            fs::remove_file(&older)?;
        }
        if old.exists() {
            fs::rename(&old, older)?;
        }
        fs::rename(&path, old)?;
    }
    // 清理超出保留份数的历史日志
    cleanup(logs_dir)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    Ok(())
}

fn cleanup(logs_dir: &Path) -> Result<()> {
    let mut files: Vec<_> = fs::read_dir(logs_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("app.log.jsonl")
                || name.starts_with("login-diagnostics.jsonl")
                || name.starts_with("track-downloads.jsonl")
        })
        .collect();
    files.sort();
    while files.len() > MAX_FILES {
        if let Some(oldest) = files.first() {
            let _ = fs::remove_file(oldest);
        }
        files.remove(0);
    }
    Ok(())
}
