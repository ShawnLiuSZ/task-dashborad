use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use rusqlite::Connection;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

mod commands;
pub mod db;
mod github;
mod mcp;
mod oauth;
mod sync;

pub struct AppState {
    pub db: Mutex<Connection>,
}

const TRAY_ID: &str = "main";
pub const SYNCED_EVENT: &str = "taskboard://synced";

fn schedule_minutes(app: &AppHandle) -> u64 {
    let state = app.state::<AppState>();
    let mins = match state.db.lock() {
        Ok(conn) => db::get_setting(&conn, "schedule_minutes")
            .parse::<u64>()
            .unwrap_or(60)
            .max(5),
        Err(_) => 60,
    };
    mins
}

fn toggle_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

fn refresh_tray(app: &AppHandle) {
    let count: i64 = {
        let state = app.state::<AppState>();
        let n = match state.db.lock() {
            Ok(conn) => conn
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE status = 'doing' AND candidate_done = 0",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            Err(_) => 0,
        };
        n
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_title(if count > 0 { Some(count.to_string()) } else { None });
        let _ = tray.set_tooltip(Some(format!("TaskBoard · 处理中 {}", count)));
    }
}

/// 执行一次同步，并刷新菜单栏角标、通知前端刷新列表。
pub fn run_sync(app: &AppHandle) -> Option<sync::SyncResult> {
    let state = app.state::<AppState>();
    // v0.3.15：同步前先看 PAT 是否存在；不存在则跳过本次、记错误、清错误信息。
    // 之所以跳过而非报错：避免自动同步在用户未配置时反复循环报错刷屏。
    let pat_present = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[sync] db lock 失败: {}", e);
                return None;
            }
        };
        !db::get_setting(&conn, "pat_token").is_empty()
    };
    if !pat_present {
        let conn = state.db.lock().ok()?;
        let _ = db::set_setting(
            &conn,
            "last_sync_error",
            "未配置 GitHub PAT，请在设置面板粘贴 token（fine-grained 推荐）",
        );
        return None;
    }
    let result = {
        let conn = match state.db.lock() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[sync] db lock 失败: {}", e);
                return None;
            }
        };
        match sync::run(&conn) {
            Ok(r) => {
                // 成功同步：清掉旧错误信息，banner 自动消失。
                let _ = db::set_setting(&conn, "last_sync_error", "");
                Some(r)
            }
            Err(e) => {
                eprintln!("[sync] 同步失败: {}", e);
                let _ = db::set_setting(&conn, "last_sync_error", &e);
                None
            }
        }
    };
    refresh_tray(app);
    if let Some(res) = result {
        let _ = app.emit(SYNCED_EVENT, res.clone());
        Some(res)
    } else {
        None
    }
}

/// MCP 子命令入口：argv 含 `mcp` 时由 `main.rs` 调用，以 stdio JSON-RPC 进程运行，
/// 复用与 GUI 相同的本地 SQLite 数据库，不启动窗口。
pub fn run_mcp() {
    mcp::run();
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let conn = db::init(&handle).map_err(|e| {
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, e))
                    as Box<dyn std::error::Error>
            })?;
            app.manage(AppState {
                db: Mutex::new(conn),
            });

            let show_item = MenuItem::with_id(app, "show", "显示看板", true, None::<&str>)?;
            let sync_item = MenuItem::with_id(app, "sync", "立即同步", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &sync_item, &quit_item])?;

            let mut builder = TrayIconBuilder::with_id(TRAY_ID)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => toggle_window(app),
                    "sync" => {
                        let h = app.clone();
                        thread::spawn(move || {
                            run_sync(&h);
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_window(tray.app_handle());
                    }
                });

            if let Some(icon) = app.default_window_icon().cloned() {
                builder = builder.icon(icon);
            }
            builder.build(app)?;

            let h_startup = handle.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(2));
                run_sync(&h_startup);
            });

            let h_tick = handle.clone();
            thread::spawn(move || loop {
                let mins = schedule_minutes(&h_tick);
                thread::sleep(Duration::from_secs(mins * 60));
                run_sync(&h_tick);
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_tasks,
            commands::sync_now,
            commands::update_task_status,
            commands::record_session,
            commands::clear_session,
            commands::record_handoff,
            commands::get_settings,
            commands::save_settings,
            commands::open_in_browser,
            // v0.3.15：PAT 管理（保留兼容，单账号视图仍可用）。
            commands::save_pat,
            commands::test_pat,
            commands::clear_pat,
            // v0.3.16+：多账号管理。
            commands::list_accounts,
            commands::add_account,
            commands::update_account,
            commands::delete_account,
            commands::test_account_pat,
            commands::set_default_account,
            commands::set_active_account,
            commands::set_view_mode,
            // v0.3.17+：GitHub OAuth Device Flow 登录。
            commands::save_oauth_client_id,
            commands::device_login_start,
            commands::device_login_poll,
            // v0.3.19+：关于页面 —— 当前版本号 + 检查更新。
            commands::get_app_version,
            commands::check_latest_release,
            // v0.3.20+：Label→Status 映射管理。
            commands::list_label_mappings,
            commands::upsert_label_mapping,
            commands::delete_label_mapping,
            // v0.3.21+：Label 列视图 + 看板模式切换。
            commands::get_label_columns_for_account,
            commands::set_board_mode,
            // v0.3.22+：Project Status 诊断。
            commands::diagnose_project_status,
            commands::list_projects,
            commands::list_project_statuses,
        ])
        .run(tauri::generate_context!())
        .expect("TaskBoard 启动失败");
}
