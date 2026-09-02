use anyhow::Result;
use chrono::Local;
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

const MAX_LOG_SIZE_BYTES: u64 = 2 * 1024 * 1024;
const LOG_FILE: &str = "login-diagnostics.jsonl";

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginDiagnostic {
    pub timestamp: String,
    pub event: String,
    pub endpoint: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qr_state: Option<String>,
    pub proxy_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_cookie_observed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cookie_persisted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_attempt: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_limit: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<String>,
}

impl LoginDiagnostic {
    pub fn new(event: &str, endpoint: &str, outcome: &str, proxy_configured: bool) -> Self {
        Self {
            timestamp: Local::now().to_rfc3339(),
            event: event.into(),
            endpoint: endpoint.into(),
            outcome: outcome.into(),
            proxy_configured,
            ..Self::default()
        }
    }
}

pub fn log_dir(logs_dir: &Path) -> Result<()> {
    fs::create_dir_all(logs_dir)?;
    Ok(())
}

pub fn append(logs_dir: &Path, entry: LoginDiagnostic) -> Result<()> {
    log_dir(logs_dir)?;
    let path = logs_dir.join(LOG_FILE);
    rotate_if_needed(&path)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    Ok(())
}

fn rotate_if_needed(path: &Path) -> Result<()> {
    if path.is_file() && fs::metadata(path)?.len() >= MAX_LOG_SIZE_BYTES {
        let previous = path.with_extension("jsonl.1");
        if previous.exists() {
            fs::remove_file(&previous)?;
        }
        fs::rename(path, previous)?;
    }
    Ok(())
}

pub fn cookie_kind(cookie: Option<&str>) -> &'static str {
    match cookie {
        Some(value)
            if value
                .split(';')
                .any(|part| part.trim_start().starts_with("MUSIC_U=")) =>
        {
            "music_u"
        }
        Some(value)
            if value
                .split(';')
                .any(|part| part.trim_start().starts_with("MUSIC_A=")) =>
        {
            "music_a"
        }
        _ => "none",
    }
}

pub fn error_class(code: &str) -> String {
    if code.is_empty() {
        "unknown".into()
    } else {
        code.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_serialization_excludes_session_secrets() {
        let mut entry = LoginDiagnostic::new("authorized", "/login/qr/check", "pending", true);
        entry.session_cookie_observed = Some(cookie_kind(Some("MUSIC_U=secret-value")).into());
        let encoded = serde_json::to_string(&entry).unwrap();
        assert!(encoded.contains("music_u"));
        assert!(!encoded.contains("secret-value"));
        assert!(!encoded.contains("unikey"));
        assert!(!encoded.contains("qrimg"));
    }

    #[test]
    fn appends_json_line() {
        let dir = tempfile::tempdir().unwrap();
        append(
            dir.path(),
            LoginDiagnostic::new("status_pending", "/login/status", "pending", false),
        )
        .unwrap();
        let contents = fs::read_to_string(dir.path().join(LOG_FILE)).unwrap();
        assert!(contents.ends_with('\n'));
        assert!(contents.contains("status_pending"));
    }

    #[test]
    fn classifies_safe_error_categories() {
        assert_eq!(error_class("http_403"), "http_403");
        assert_eq!(error_class("timeout"), "timeout");
        assert_eq!(error_class(""), "unknown");
    }
}
