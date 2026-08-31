use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const APP_QUALIFIER: &str = "com";
const APP_ORGANIZATION: &str = "muxinxy";
const APP_NAME: &str = "music-auto-sync";
const PORTABLE_MARKER: &str = "portable.ini";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub database_file: PathBuf,
    pub cache_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub portable: bool,
    pub exe_dir: PathBuf,
}

impl DataPaths {
    pub fn discover() -> Result<Self> {
        let exe = env::current_exe().context("cannot resolve executable path")?;
        let exe_dir = exe.parent().context("executable has no parent directory")?.to_path_buf();

        if let Some(path) = Self::arg_data_dir() {
            return Self::from_root(path, false, exe_dir);
        }

        let marker = exe_dir.join(PORTABLE_MARKER);
        if marker.is_file() {
            let configured = fs::read_to_string(&marker)
                .context("cannot read portable.ini")?
                .trim()
                .to_owned();
            if !configured.is_empty() {
                return Self::from_root(PathBuf::from(configured), true, exe_dir);
            }
        }

        let portable_data = exe_dir.join("data");
        if portable_data.is_dir() {
            return Self::from_root(portable_data, true, exe_dir);
        }

        let dirs = ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_NAME)
            .context("cannot determine OS application data directory")?;
        Self::from_root(dirs.data_local_dir().to_path_buf(), false, exe_dir)
    }

    pub fn from_root(root: PathBuf, portable: bool, exe_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).with_context(|| format!("cannot create data directory: {}", root.display()))?;
        let cache_dir = root.join("cache");
        let logs_dir = root.join("logs");
        fs::create_dir_all(&cache_dir)?;
        fs::create_dir_all(&logs_dir)?;
        Ok(Self {
            config_file: root.join("config.json"),
            database_file: root.join("library.db"),
            root,
            cache_dir,
            logs_dir,
            portable,
            exe_dir,
        })
    }

    pub fn write_portable_marker(&self, root: &Path) -> Result<()> {
        fs::write(self.exe_dir.join(PORTABLE_MARKER), root.to_string_lossy().as_bytes())
            .context("cannot write portable.ini")
    }

    fn arg_data_dir() -> Option<PathBuf> {
        let mut args = env::args_os();
        while let Some(arg) = args.next() {
            let arg = arg.to_string_lossy();
            if let Some(path) = arg.strip_prefix("--data-dir=") {
                return Some(PathBuf::from(path));
            }
            if arg == "--data-dir" {
                return args.next().map(PathBuf::from);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_data_layout() {
        let temp = tempfile::tempdir().unwrap();
        let p = DataPaths::from_root(temp.path().join("state"), true, temp.path().to_path_buf()).unwrap();
        assert!(p.config_file.parent().unwrap().is_dir());
        assert!(p.cache_dir.is_dir());
        assert!(p.logs_dir.is_dir());
    }
}
