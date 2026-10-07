//! 任务结束的系统通知（歌单同步 / 云盘 / 工具箱后台任务）。
//!
//! 文案按 config.language 中英二选一（与托盘同一约定，不引入 i18n 框架）；
//! 是否通知由 config.notify_on_task_done 控制（默认开启），读不到配置时一律
//! 不打扰。通知发送失败只记日志，绝不影响任务本身。CLI 模式（无 AppHandle
//! 参与的 cli_sync）不经过这里。

use tauri::{AppHandle, Manager};
#[cfg(not(target_os = "windows"))]
use tauri_plugin_notification::NotificationExt;

use crate::{core::sync::SyncReport, store, AppState};

/// 便携版没有开始菜单快捷方式（NSIS 安装版由安装器注册），而 Windows 的
/// WinRT toast 只认注册过的 AppUserModelID——未注册时通知**静默不显示**。
/// 启动时在 HKCU 注册 AUMID（等同当前用户级快捷方式的效果，无需管理员），
/// 与 tauri-plugin-notification 使用的 AUMID（config.identifier）保持一致。
#[cfg(target_os = "windows")]
pub fn register_aumid(app: &AppHandle) {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let identifier = app.config().identifier.clone();
    let display_name = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| "Music Auto Sync".into());
    let icon = std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    let cu = match RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(format!(r"Software\Classes\AppUserModelId\{identifier}"))
    {
        Ok((key, _)) => key,
        Err(error) => {
            tracing::warn!(%error, "notification AUMID registration failed");
            return;
        }
    };
    let _ = cu.set_value("DisplayName", &display_name);
    let _ = cu.set_value("IconUri", &icon);
    let _ = cu.set_value("ShowInSettings", &1u32);
}

/// 返回 (是否启用通知, 是否英文)。配置读取失败时返回 None（不打扰）。
fn settings(app: &AppHandle) -> Option<(bool, bool)> {
    let state = app.state::<AppState>();
    store::config::load(&state.paths.get().config_file)
        .ok()
        .map(|config| {
            (
                config.notify_on_task_done,
                config.language.starts_with("en"),
            )
        })
}

fn show(app: &AppHandle, title: String, body: String) {
    // Windows 直接构造 toast 以挂 on_activated 回调：点击通知唤起主窗口
    // （插件的 builder 不暴露激活回调）。窗口 show/focus 是向事件循环投递
    // 消息，从 WinRT 回调线程调用安全。
    #[cfg(target_os = "windows")]
    {
        let handle = app.clone();
        let toast = tauri_winrt_notification::Toast::new(&app.config().identifier)
            .title(&title)
            .text1(&body)
            .on_activated(move |_| {
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                Ok(())
            });
        if let Err(error) = toast.show() {
            tracing::warn!(%error, "notification failed");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Err(error) = app.notification().builder().title(title).body(body).show() {
            tracing::warn!(%error, "notification failed");
        }
    }
}

/// 哨兵任务名与工具 kind → 显示名；歌单名原样返回。
fn task_name(en: bool, name: &str) -> String {
    match name {
        "cloud" => (if en { "Cloud upload" } else { "云盘上传" }).into(),
        "cloud_download" => (if en { "Cloud download" } else { "云盘下载" }).into(),
        "repair" => (if en { "Tag repair" } else { "属性修复" }).into(),
        "ncm_convert" => (if en { "NCM conversion" } else { "NCM 转换" }).into(),
        "cleanup_scan" => (if en { "Duplicate scan" } else { "重复扫描" }).into(),
        other => other.to_string(),
    }
}

fn title(en: bool, canceled: bool, task_failed: bool, partial: bool) -> String {
    if canceled {
        (if en { "Task canceled" } else { "任务已取消" }).into()
    } else if task_failed {
        (if en { "Task failed" } else { "任务失败" }).into()
    } else if partial {
        (if en { "Task finished with failures" } else { "任务部分成功" }).into()
    } else {
        (if en { "Task finished" } else { "任务完成" }).into()
    }
}

/// 歌单/云盘类任务结束：计数 = (新增, 隔离, 失败)。
/// canceled/task_failed 表达异常结局（云盘任务 report 里也带已累计的计数）。
pub fn sync_finished(
    app: &AppHandle,
    raw_name: &str,
    added: usize,
    quarantined: usize,
    failed: usize,
    canceled: bool,
    task_failed: bool,
) {
    let Some((enabled, en)) = settings(app) else { return };
    if !enabled {
        return;
    }
    let name = task_name(en, raw_name);
    let body = if en {
        format!("{name}: added {added} · quarantined {quarantined} · failed {failed}")
    } else {
        format!("{name}：新增 {added} · 隔离 {quarantined} · 失败 {failed}")
    };
    show(
        app,
        title(en, canceled, task_failed, failed > 0 && !canceled && !task_failed),
        body,
    );
}

/// 多歌单批量同步结束：汇总一次，避免每个歌单弹一条。
pub fn sync_batch(app: &AppHandle, reports: &[SyncReport], failed_runs: usize) {
    let Some((enabled, en)) = settings(app) else { return };
    if !enabled {
        return;
    }
    let added: usize = reports.iter().map(|r| r.added).sum();
    let quarantined: usize = reports.iter().map(|r| r.quarantined).sum();
    let failed: usize = reports.iter().map(|r| r.failed).sum();
    let partial = failed > 0 || failed_runs > 0;
    let mut body = if en {
        format!("{} playlist(s): added {added} · quarantined {quarantined} · failed {failed}", reports.len())
    } else {
        format!("{} 个歌单：新增 {added} · 隔离 {quarantined} · 失败 {failed}", reports.len())
    };
    if failed_runs > 0 {
        let suffix = if en {
            format!(" · {failed_runs} playlist run(s) failed")
        } else {
            format!(" · {failed_runs} 个歌单同步失败")
        };
        body.push_str(&suffix);
    }
    show(app, title(en, false, false, partial), body);
}

/// 任务级失败（无报告可计数时）：歌单同步整体失败等。
pub fn task_failed(app: &AppHandle, which: Option<&str>) {
    let Some((enabled, en)) = settings(app) else { return };
    if !enabled {
        return;
    }
    let body = match which {
        Some(name) => task_name(en, name),
        None => if en {
            "See the Sync page logs for details".into()
        } else {
            "详见同步页的任务日志".into()
        },
    };
    show(app, title(en, false, true, false), body);
}

/// 工具箱任务结束：计数 = (成功, 跳过, 失败)。
pub fn tool_finished(
    app: &AppHandle,
    kind: &str,
    ok: usize,
    skipped: usize,
    failed: usize,
    canceled: bool,
    task_failed: bool,
) {
    let Some((enabled, en)) = settings(app) else { return };
    if !enabled {
        return;
    }
    let name = task_name(en, kind);
    let body = if en {
        format!("{name}: {ok} ok · {skipped} skipped · {failed} failed")
    } else {
        format!("{name}：成功 {ok} · 跳过 {skipped} · 失败 {failed}")
    };
    show(
        app,
        title(en, canceled, task_failed, failed > 0 && ok > 0 && !canceled && !task_failed),
        body,
    );
}
