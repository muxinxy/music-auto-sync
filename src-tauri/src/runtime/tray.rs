use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
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

fn is_paused(app: &AppHandle) -> bool {
    app.state::<AppState>().pause_requested.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    // 重建前移除旧托盘图标，避免重复图标（例如切换语言后重装）。
    let _ = app.remove_tray_by_id("main-tray");
    let en = language(app);
    let paused = is_paused(app);
    let running = app
        .state::<AppState>()
        .sync_running
        .load(std::sync::atomic::Ordering::SeqCst);
    let show_label = if en { "Show window" } else { "打开主窗口" };
    let sync_label = if en { "Sync now" } else { "立即同步" };
    let pause_label = if en { "Pause" } else { "暂停" };
    let resume_label = if en { "Resume" } else { "继续" };
    let cancel_label = if en { "Cancel" } else { "取消" };
    let quit_label = if en { "Quit" } else { "退出" };
    let tooltip = if en { "Music Auto Sync" } else { "音乐同步" };

    let show = MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", sync_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    // 暂停/继续/取消仅在任务进行时显示（避免空任务点击导致异常）。
    let menu = if !running {
        Menu::with_items(app, &[&show, &sync, &quit])?
    } else if paused {
        let resume = MenuItem::with_id(app, "resume", resume_label, true, None::<&str>)?;
        let cancel = MenuItem::with_id(app, "cancel", cancel_label, true, None::<&str>)?;
        Menu::with_items(app, &[&show, &sync, &resume, &cancel, &quit])?
    } else {
        let pause = MenuItem::with_id(app, "pause", pause_label, true, None::<&str>)?;
        let cancel = MenuItem::with_id(app, "cancel", cancel_label, true, None::<&str>)?;
        Menu::with_items(app, &[&show, &sync, &pause, &cancel, &quit])?
    };

    let mut tray = TrayIconBuilder::with_id("main-tray")
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            // 左键单击打开主窗口；右键才弹菜单（默认行为由 show_menu_on_left_click(false) 保证）。
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
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
                    if let Err(error) = sync::sync_enabled_with_source(&app, &state, "tray").await {
                        tracing::warn!(%error, "tray synchronization failed");
                    }
                    let _ = install(&app);
                });
            }
            "pause" => {
                let state = app.state::<AppState>();
                if state.sync_running.load(std::sync::atomic::Ordering::SeqCst) {
                    state
                        .pause_requested
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                let _ = install(app);
            }
            "resume" => {
                let state = app.state::<AppState>();
                state
                    .pause_requested
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = install(app);
            }
            "cancel" => {
                let state = app.state::<AppState>();
                state.cancel_requested.store(true, std::sync::atomic::Ordering::SeqCst);
                state
                    .pause_requested
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = install(app);
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
