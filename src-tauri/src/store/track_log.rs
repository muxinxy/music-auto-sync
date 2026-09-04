use anyhow::Result;
use chrono::Local;
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use crate::error::UiMessage;

const LOG_FILE: &str = "track-downloads.jsonl";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackLogEntry {
    pub ts: String,
    pub playlist_id: Option<u64>,
    pub playlist_name: String,
    pub track_id: u64,
    pub track_name: String,
    pub outcome: String, // downloaded | skipped | failed | no_url | file_exists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TrackLogEntry {
    pub fn new(
        playlist_id: Option<u64>,
        playlist_name: &str,
        track_id: u64,
        track_name: &str,
    ) -> Self {
        Self {
            ts: Local::now().to_rfc3339(),
            playlist_id,
            playlist_name: playlist_name.to_owned(),
            track_id,
            track_name: track_name.to_owned(),
            outcome: "pending".into(),
            path: None,
            bytes: None,
            quality: None,
            error: None,
        }
    }

    pub fn done(&mut self, path: &Path, bytes: u64, quality: &str) {
        self.outcome = "downloaded".into();
        self.path = Some(path.to_string_lossy().into_owned());
        self.bytes = Some(bytes);
        self.quality = Some(quality.to_owned());
    }

    pub fn outcome(&mut self, outcome: &str, error: &UiMessage) {
        self.outcome = outcome.into();
        self.error = Some(error.to_json());
    }
}

pub fn append(logs_dir: &Path, entry: &TrackLogEntry) -> Result<()> {
    fs::create_dir_all(logs_dir)?;
    let path: PathBuf = logs_dir.join(LOG_FILE);
    if path.is_file() && fs::metadata(&path)?.len() >= 5 * 1024 * 1024 {
        let previous = path.with_extension("jsonl.1");
        if previous.exists() {
            fs::remove_file(&previous)?;
        }
        fs::rename(&path, previous)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(entry)?)?;
    Ok(())
}
