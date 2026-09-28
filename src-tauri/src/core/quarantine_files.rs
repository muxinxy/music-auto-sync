//! 隔离文件的防扫描处理：文件名追加 `.quarantined` 后缀 + Windows 隐藏属性。
//!
//! 网易云客户端等按扩展名扫描音乐库，未知后缀不会被收录；隐藏属性作为第二道
//! 保险（资源管理器中显示为灰色，仍可见可管理）。恢复时去掉隐藏并改名回原路径。

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

/// 追加在隔离文件名末尾的后缀：`a.mp3` → `a.mp3.quarantined`。
pub(crate) const QUARANTINED_SUFFIX: &str = "quarantined";

pub(crate) fn quarantined_filename(file_name: &str) -> String {
    format!("{file_name}.{QUARANTINED_SUFFIX}")
}

/// 判断路径是否已是隔离命名（最后一段扩展名是 .quarantined）。
pub(crate) fn is_quarantined(path: &Path) -> bool {
    path.extension().and_then(|x| x.to_str()) == Some(QUARANTINED_SUFFIX)
}

/// 设置/清除 Windows 隐藏属性；非 Windows 平台为空操作。
#[cfg(windows)]
pub(crate) fn set_hidden(path: &Path, hidden: bool) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL,
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let flags = if hidden {
        FILE_ATTRIBUTE_HIDDEN
    } else {
        FILE_ATTRIBUTE_NORMAL
    };
    unsafe {
        SetFileAttributesW(wide.as_ptr(), flags);
    }
}

#[cfg(not(windows))]
pub(crate) fn set_hidden(_path: &Path, _hidden: bool) {}

/// 一次性迁移：把旧格式（无 `.quarantined` 后缀）的隔离文件改成新命名并补隐藏
/// 属性，同步更新 `quarantine` 与 `deleted_log` 两张表里的路径。最后兜底扫描
/// `music_root/.quarantine` 下已无数据库记录的孤儿文件。幂等，在隔离区列表
/// 加载时调用。返回实际迁移的文件数。
pub(crate) fn migrate_existing(conn: &rusqlite::Connection, music_root: Option<&Path>) -> usize {
    let mut migrated = 0;
    // 1) quarantine 表引用的文件。
    migrated += migrate_quarantine_table(conn);
    // 2) deleted_log 引用、但隔离行已被消费的文件。
    migrated += migrate_deleted_log(conn);
    // 3) 兜底：无任何数据库记录的孤儿文件（记录在历史测试中被恢复/清空）。
    if let Some(root) = music_root {
        migrated += sweep_orphans(root);
    }
    migrated
}

fn migrate_quarantine_table(conn: &rusqlite::Connection) -> usize {
    let Ok(mut stmt) = conn.prepare("SELECT id, quarantine_path FROM quarantine") else {
        return 0;
    };
    let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))) else {
        return 0;
    };
    let mut migrated = 0;
    for row in rows.flatten() {
        let (id, old_path) = row;
        let path = PathBuf::from(&old_path);
        if !path.is_file() {
            continue;
        }
        if is_quarantined(&path) {
            set_hidden(&path, true);
            continue;
        }
        let Some(new_path) = rename_to_quarantined(&path) else {
            continue;
        };
        set_hidden(Path::new(&new_path), true);
        let _ = conn.execute(
            "UPDATE quarantine SET quarantine_path=?1 WHERE id=?2",
            rusqlite::params![new_path, id],
        );
        // 删除日志可能引用同一文件（清理工具路径），一并更新。
        let _ = conn.execute(
            "UPDATE deleted_log SET quarantined_path=?1 WHERE quarantined_path=?2",
            rusqlite::params![new_path, old_path],
        );
        migrated += 1;
    }
    migrated
}

fn migrate_deleted_log(conn: &rusqlite::Connection) -> usize {
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, quarantined_path FROM deleted_log
         WHERE quarantined_path IS NOT NULL AND restored_at IS NULL",
    ) else {
        return 0;
    };
    let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))) else {
        return 0;
    };
    let mut migrated = 0;
    for row in rows.flatten() {
        let (id, old_path) = row;
        let path = PathBuf::from(&old_path);
        // quarantine 表已处理过（带后缀）的跳过；只处理它漏掉的引用。
        if !path.is_file() || is_quarantined(&path) {
            continue;
        }
        let Some(new_path) = rename_to_quarantined(&path) else {
            continue;
        };
        set_hidden(Path::new(&new_path), true);
        let _ = conn.execute(
            "UPDATE deleted_log SET quarantined_path=?1 WHERE id=?2",
            rusqlite::params![new_path, id],
        );
        migrated += 1;
    }
    migrated
}

/// 扫描孤儿裸音频：音乐根目录自身的 `.quarantine` 及其一级子文件夹的
/// `.quarantine`（清理工具与同步引擎的实际落点）。找到的不带后缀音频
/// 改名 + 隐藏。无数据库记录可更新，仅消除客户端可见性；恢复需手动改回文件名。
fn sweep_orphans(root: &Path) -> usize {
    let mut bases: Vec<PathBuf> = vec![root.join(".quarantine")];
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                bases.push(child.join(".quarantine"));
            }
        }
    }
    let mut migrated = 0;
    for base in bases {
        migrated += sweep_base(&base);
    }
    migrated
}

fn sweep_base(base: &Path) -> usize {
    if !base.is_dir() {
        return 0;
    }
    let mut migrated = 0;
    for entry in WalkDir::new(base)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() || is_quarantined(path) {
            continue;
        }
        let is_audio = path
            .extension()
            .and_then(|x| x.to_str())
            .map(|x| crate::core::cleanup::AUDIO_EXTS.contains(&x.to_ascii_lowercase().as_str()))
            .unwrap_or(false);
        if !is_audio {
            continue;
        }
        if rename_to_quarantined(path).is_some() {
            let renamed = path.with_file_name(quarantined_filename(
                path.file_name().and_then(|x| x.to_str()).unwrap_or_default(),
            ));
            set_hidden(&renamed, true);
            migrated += 1;
        }
    }
    migrated
}

/// 把单个文件改名为追加了 `.quarantined` 后缀的新路径；目标已存在或改名失败返回 None。
fn rename_to_quarantined(path: &Path) -> Option<String> {
    let new_name = quarantined_filename(
        path.file_name().and_then(|x| x.to_str()).unwrap_or_default(),
    );
    let target = path.with_file_name(new_name);
    if target.exists() {
        return None;
    }
    std::fs::rename(path, &target).ok()?;
    Some(target.to_string_lossy().into_owned())
}
