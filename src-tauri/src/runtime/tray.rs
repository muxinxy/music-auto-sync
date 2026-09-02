use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

use crate::{core::sync, store, AppState};

fn language(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    match store::config::load(&state.paths.get().config_file) {
        Ok(config) => config.language.starts_with("en"),
        Err(_) => false,
    }
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    // 重建前移除旧托盘图标，避免重复图标（例如切换语言后重装）。
    let _ = app.remove_tray_by_id("main-tray");
    let en = language(app);
    let show_label = if en { "Show window" } else { "打开主窗口" };
    let sync_label = if en { "Sync now" } else { "立即同步" };
    let quit_label = if en { "Quit" } else { "退出" };
    let tooltip = if en {
        "Music Auto Sync"
    } else {
        "音乐同步"
    };

    let show = MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", sync_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &sync, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main-tray")
        .tooltip(tooltip)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "sync" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    if let Err(error) = sync::sync_enabled(&app, &state).await {
                        tracing::warn!(%error, "tray synchronization failed");
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
