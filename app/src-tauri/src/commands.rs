use rusqlite::Connection;
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

use crate::db::Account;
use crate::sync::SyncResult;
use crate::AppState;

const VALID_STATUS: &[&str] = &["todo", "doing", "processed", "done"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub key: String,
    pub owner: String,
    pub repo: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub gh_state: String,
    pub ownership: String,
    pub status: String,
    pub gh_status: String,
    pub assignees: String,
    pub mentioned: bool,
    pub latest_comment_url: String,
    pub pr_number: i64,
    pub pr_url: String,
    pub branch: String,
    pub session_id: Option<String>,
    pub session_agent: Option<String>,
    pub session_at: Option<i64>,
    pub candidate_done: bool,
    pub handoff: String,
    pub updated_at: Option<String>,
    /// v0.3.16：归属账号 id（指向 accounts.id），用于多账号视图过滤。
    pub account_id: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schedule_minutes: u64,
    /// 历史字段：保留兼容（v0.3.15 起不再用于任何路径，仅显示）。
    pub gh_path: String,
    pub login: String,
    pub org: String,
    pub last_sync_at: i64,
    pub db_path: String,
    /// v0.3.15+：是否已配置 PAT（不回显 token 本体）。
    pub has_pat: bool,
    /// v0.3.15+：最近一次同步的错误信息（如「未配置 PAT」「GitHub 401…」）。
    /// 当作 banner 展示给用户，便于诊断；成功同步时清空。
    pub last_sync_error: String,
    /// v0.3.16+：当前激活账号 id（单账号视图下同步此账号）。
    pub active_account_id: i64,
    /// v0.3.16+：视图模式。'single'=仅当前激活账号；'all'=所有账号任务聚合。
    pub view_mode: String,
    /// v0.3.16+：所有账号列表（不含 PAT 本体）。
    pub accounts: Vec<Account>,
    /// v0.3.17+：GitHub OAuth Device Flow 的 client_id（注册 OAuth App 后填一次）。
    pub oauth_client_id: String,
}

fn rows_to_tasks(conn: &Connection, ownership: Option<&str>, account_filter: Option<i64>) -> Result<Vec<Task>, String> {
    // v0.3.16：account_filter 解析。
    // - None / Some(n>0) → 按指定账号 id 过滤
    // - Some(0) → 不加 account_id 条件（即"全部账号"聚合视图）
    let (where_extra, use_account_filter, account_id) = match account_filter {
        Some(0) => (String::new(), false, 0i64),
        Some(n) => (" AND account_id = ?".to_string(), true, n),
        None => {
            // 默认：单账号视图（active_account_id）；若无任何账号则不过滤（空集）。
            let active: i64 = conn
                .query_row(
                    "SELECT value FROM meta WHERE key = 'active_account_id'",
                    [],
                    |r| r.get::<_, String>(0),
                )
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if active > 0 {
                (" AND account_id = ?".to_string(), true, active)
            } else {
                (String::new(), false, 0i64)
            }
        }
    };
    let (sql, use_ownership_filter) = match ownership {
        Some(_) => (
            format!(
                "SELECT key, owner, repo, number, title, url, gh_state, ownership, status, gh_status,
                        assignees, mentioned, latest_comment_url, pr_number, pr_url, branch,
                        session_id, session_agent, session_at, candidate_done, handoff, updated_at, account_id
                 FROM tasks WHERE ownership = ?{where_extra}
                 ORDER BY candidate_done ASC, status ASC, updated_at DESC"
            ),
            true,
        ),
        None => (
            format!(
                "SELECT key, owner, repo, number, title, url, gh_state, ownership, status, gh_status,
                        assignees, mentioned, latest_comment_url, pr_number, pr_url, branch,
                        session_id, session_agent, session_at, candidate_done, handoff, updated_at, account_id
                 FROM tasks WHERE 1=1{where_extra}
                 ORDER BY candidate_done ASC, status ASC, updated_at DESC"
            ),
            false,
        ),
    };

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mapper = |r: &rusqlite::Row| {
        Ok(Task {
            key: r.get(0)?,
            owner: r.get(1)?,
            repo: r.get(2)?,
            number: r.get(3)?,
            title: r.get(4)?,
            url: r.get(5)?,
            gh_state: r.get(6)?,
            ownership: r.get(7)?,
            status: r.get(8)?,
            gh_status: r.get(9)?,
            assignees: r.get(10)?,
            mentioned: r.get::<_, i64>(11).unwrap_or(0) != 0,
            latest_comment_url: r.get(12)?,
            pr_number: r.get(13)?,
            pr_url: r.get(14)?,
            branch: r.get(15)?,
            session_id: r.get(16)?,
            session_agent: r.get(17)?,
            session_at: r.get(18)?,
            candidate_done: r.get::<_, i64>(19).unwrap_or(0) != 0,
            handoff: r.get(20)?,
            updated_at: r.get(21)?,
            account_id: r.get(22)?,
        })
    };

    // 动态参数：归属 + account_id（按 use_*_filter 标志决定传几个）
    let rows: rusqlite::Result<rusqlite::MappedRows<'_, _>> = match (use_ownership_filter, use_account_filter) {
        (true, true) => stmt.query_map(
            rusqlite::params![ownership.unwrap_or(""), account_id],
            mapper,
        ),
        (true, false) => stmt.query_map(rusqlite::params![ownership.unwrap_or("")], mapper),
        (false, true) => stmt.query_map(rusqlite::params![account_id], mapper),
        (false, false) => stmt.query_map([], mapper),
    };
    let mut out = Vec::new();
    for r in rows.map_err(|e| e.to_string())? {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 列出任务；v0.3.16 起支持按账号过滤。
///
/// `account_id` 约定：
/// - `None` → 单账号视图（默认）；后端读 `meta.active_account_id`
/// - `Some(0)` → 全部账号聚合视图
/// - `Some(n>0)` → 指定账号 id
#[tauri::command]
pub fn list_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    ownership: Option<String>,
    account_id: Option<i64>,
) -> Result<Vec<Task>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let _ = app;
    rows_to_tasks(&conn, ownership.as_deref(), account_id)
}

#[tauri::command]
pub async fn sync_now(app: AppHandle) -> Result<SyncResult, String> {
    // 同步要跑 5 次 Search API + 1 次 GraphQL（5~15s）。若作为同步命令直接跑，
    // 会阻塞 Tauri 主线程（事件循环），表现为 macOS 转圈 beachball、UI 看似卡死。
    // 改为 async + spawn_blocking：主线程仅派发后立即返回，重活在工作线程执行，
    // UI 的「同步中…」指示与渲染不受影响。
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let st = handle.state::<AppState>();
        let conn = st.db.lock().map_err(|e| e.to_string())?;
        crate::sync::run(&conn)
    })
    .await
    .map_err(|e| format!("同步线程异常: {}", e))?
}

#[tauri::command]
pub fn update_task_status(
    state: State<'_, AppState>,
    key: String,
    status: String,
) -> Result<(), String> {
    if !VALID_STATUS.contains(&status.as_str()) {
        return Err(format!("非法状态: {}", status));
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE tasks SET status = ?1 WHERE key = ?2",
        rusqlite::params![status, key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn record_session(
    state: State<'_, AppState>,
    key: String,
    session_id: String,
    agent: Option<String>,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let now = crate::sync::now_secs();
    conn.execute(
        "UPDATE tasks SET session_id = ?1, session_agent = ?2, session_at = ?3 WHERE key = ?4",
        rusqlite::params![session_id, agent.unwrap_or_default(), now, key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn clear_session(state: State<'_, AppState>, key: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE tasks SET session_id = NULL, session_agent = NULL WHERE key = ?1",
        [key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 记录「交接任务」详情：由接入的 agent（claude / codex 等）在识别到用户「生成交接任务」类意图时调用，
/// 把交接上下文写入该 issue 卡片的 handoff 字段，供后续接手者直接在详情页查看。
#[tauri::command]
pub fn record_handoff(
    state: State<'_, AppState>,
    key: String,
    text: String,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE tasks SET handoff = ?1 WHERE key = ?2",
        rusqlite::params![text, key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_settings(app: AppHandle, state: State<'_, AppState>) -> Result<Settings, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let db_path = crate::db::db_path(&app).map_err(|e| e.to_string())?;
    let accounts = crate::db::list_accounts(&conn)?;
    let active_account_id: i64 = crate::db::get_setting(&conn, "active_account_id")
        .parse()
        .unwrap_or(0);
    let view_mode = crate::db::get_setting(&conn, "view_mode");
    let view_mode = if view_mode.is_empty() { "single".to_string() } else { view_mode };
    Ok(Settings {
        schedule_minutes: crate::db::get_setting(&conn, "schedule_minutes")
            .parse::<u64>()
            .unwrap_or(60)
            .max(5),
        gh_path: crate::db::get_setting(&conn, "gh_path"),
        login: crate::db::get_setting(&conn, "login"),
        org: crate::db::get_setting(&conn, "org"),
        last_sync_at: crate::db::get_setting(&conn, "last_sync_at")
            .parse::<i64>()
            .unwrap_or(0),
        db_path: db_path.to_string_lossy().to_string(),
        has_pat: !crate::db::get_setting(&conn, "pat_token").is_empty(),
        last_sync_error: crate::db::get_setting(&conn, "last_sync_error"),
        active_account_id,
        view_mode,
        accounts,
        oauth_client_id: crate::db::get_setting(&conn, "oauth_client_id"),
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatStatus {
    pub login: String,
    pub has_pat: bool,
}

/// 保存 GitHub Personal Access Token。空串视为清除。
/// 成功保存后立刻探测账号（`test_connection`），把 login 写入 `meta.login`，
/// 便于前端展示「当前账号」且不必泄漏 PAT 本体。
#[allow(dead_code)] // 由 `lib.rs::invoke_handler` 反射注册使用，编译期无可达调用
#[tauri::command]
pub fn save_pat(
    app: AppHandle,
    state: State<'_, AppState>,
    pat: String,
) -> Result<PatStatus, String> {
    let trimmed = pat.trim().to_string();
    if trimmed.is_empty() {
        // 用户点「清除」：清空 pat_token / login，并清掉 last_sync_error 中与 PAT 相关提示。
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        crate::db::set_setting(&conn, "pat_token", "")?;
        crate::db::set_setting(&conn, "login", "")?;
        crate::db::set_setting(&conn, "last_sync_error", "")?;
        return Ok(PatStatus { login: String::new(), has_pat: false });
    }
    // 先在锁外构造客户端（避免构造时阻塞 db 锁）；构造仅作 PAT 形式校验，
    // 真实 login 探测走 test_connection（v0.3.16 起构造不再自动探测）。
    let client = crate::github::GitHubClient::new(trimmed.clone(), String::new(), String::new())
        .map_err(|e| format!("PAT 验证失败: {}", e))?;
    let login = client.test_connection()?.login;
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        crate::db::set_setting(&conn, "pat_token", &trimmed)?;
        crate::db::set_setting(&conn, "login", &login)?;
        // 写入新 PAT 后清掉旧的同步错误——这次刷新才会用到新 token。
        crate::db::set_setting(&conn, "last_sync_error", "")?;
    }
    let _ = app;
    Ok(PatStatus { login, has_pat: true })
}

/// 测试当前已保存的 PAT 是否有效，返回账号。
/// 给设置面板「测试连接」按钮专用。
#[allow(dead_code)]
#[tauri::command]
pub fn test_pat(state: State<'_, AppState>) -> Result<PatStatus, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let pat = crate::db::get_setting(&conn, "pat_token");
    if pat.is_empty() {
        return Err("未配置 PAT，请先在设置面板粘贴".to_string());
    }
    // 取锁外构造（reqwest 的网络 IO 与 db 锁解耦）；构造后探测真实 login。
    drop(conn);
    let client = crate::github::GitHubClient::new(pat, String::new(), String::new())?;
    let probe = client.test_connection()?.login;
    Ok(PatStatus { login: probe, has_pat: true })
}

/// 清除 PAT（与 `save_pat` 传空串等价，但语义独立，便于前端显式调用）。
#[allow(dead_code)]
#[tauri::command]
pub fn clear_pat(state: State<'_, AppState>) -> Result<PatStatus, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::set_setting(&conn, "pat_token", "")?;
    crate::db::set_setting(&conn, "login", "")?;
    crate::db::set_setting(&conn, "last_sync_error", "")?;
    Ok(PatStatus { login: String::new(), has_pat: false })
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    schedule_minutes: u64,
    gh_path: String,
) -> Result<Settings, String> {
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        crate::db::set_setting(&conn, "schedule_minutes", &schedule_minutes.max(5).to_string())?;
        crate::db::set_setting(&conn, "gh_path", &gh_path)?;
    }
    get_settings(app, state)
}

#[tauri::command]
pub fn open_in_browser(url: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("打开浏览器失败: {}", e))?;
    Ok(())
}

// ============================================================================
// v0.3.17+：GitHub OAuth Device Flow 登录
// ============================================================================

/// 保存 Device Flow 用的 OAuth client_id（注册 OAuth App 后填一次即可）。
#[allow(dead_code)]
#[tauri::command]
pub fn save_oauth_client_id(state: State<'_, AppState>, client_id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::set_setting(&conn, "oauth_client_id", client_id.trim())
}

/// 第 1 步：申请设备码。client_id 空时回退到已保存值。
#[allow(dead_code)]
#[tauri::command]
pub async fn device_login_start(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<crate::oauth::DeviceLoginStart, String> {
    // 空入参 → 用已保存的；都没有则由 oauth::start 报错提示注册。
    let cid = if client_id.trim().is_empty() {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        crate::db::get_setting(&conn, "oauth_client_id")
    } else {
        client_id.trim().to_string()
    };
    tauri::async_runtime::spawn_blocking(move || crate::oauth::start(&cid))
        .await
        .map_err(|e| format!("登录任务异常: {e}"))?
}

/// 轮询结果（前端按 interval 反复调用本命令；**后端不 sleep**）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLoginPoll {
    /// pending | slow_down | success | error
    pub status: String,
    /// 成功时填：授权账号的 GitHub login。
    pub login: String,
    /// 成功时填：新建/更新的账号 id。
    pub account_id: i64,
    /// 提示或错误信息。
    pub message: String,
}

/// 第 2 步：单次轮询。成功时**在后端**探测 login 并直接建账号——token 全程不回流前端。
#[allow(dead_code)]
#[tauri::command]
pub async fn device_login_poll(
    app: AppHandle,
    client_id: String,
    device_code: String,
    org: String,
    label: String,
) -> Result<DeviceLoginPoll, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<AppState>();
        let outcome = crate::oauth::poll_once(&client_id, &device_code)?;
        match outcome {
            crate::oauth::PollOutcome::Pending => Ok(DeviceLoginPoll {
                status: "pending".to_string(),
                login: String::new(),
                account_id: 0,
                message: String::new(),
            }),
            crate::oauth::PollOutcome::SlowDown => Ok(DeviceLoginPoll {
                status: "slow_down".to_string(),
                login: String::new(),
                account_id: 0,
                message: String::new(),
            }),
            crate::oauth::PollOutcome::Failed(msg) => Ok(DeviceLoginPoll {
                status: "error".to_string(),
                login: String::new(),
                account_id: 0,
                message: msg,
            }),
            crate::oauth::PollOutcome::Success(token) => {
                // 探测真实 login（token 有效才继续）。
                let login = crate::github::GitHubClient::new(
                    token.clone(),
                    String::new(),
                    String::new(),
                )
                .and_then(|c| c.test_connection())
                .map_err(|e| format!("授权成功但探测账号失败: {}", e))?
                .login;
                // 建账号：同 login 已存在则更新 PAT（重新授权场景），否则插入。
                let conn = state.db.lock().map_err(|e| e.to_string())?;
                let existing: Option<i64> = {
                    let mut stmt = conn
                        .prepare("SELECT id FROM accounts WHERE login = ?1 ORDER BY id LIMIT 1")
                        .map_err(|e| e.to_string())?;
                    stmt.query_row([&login], |r| r.get(0)).ok()
                };
                let account_id = match existing {
                    Some(id) => {
                        crate::db::update_account(&conn, id, None, None, None, Some(&token))?;
                        id
                    }
                    None => {
                        let final_label = if label.trim().is_empty() {
                            login.clone()
                        } else {
                            label.trim().to_string()
                        };
                        crate::db::insert_account(&conn, &final_label, &login, org.trim(), &token)?
                    }
                };
                // 新账号若是首个，把 active 拨过去（与 add_account 行为一致）。
                let active: i64 = crate::db::get_setting(&conn, "active_account_id")
                    .parse()
                    .unwrap_or(0);
                if active == 0 {
                    crate::db::set_setting(&conn, "active_account_id", &account_id.to_string())?;
                }
                crate::db::set_setting(&conn, "last_sync_error", "")?;
                let _ = handle; // AppHandle 预留给后续事件通知；当前前端轮询即可。
                Ok(DeviceLoginPoll {
                    status: "success".to_string(),
                    login,
                    account_id,
                    message: String::new(),
                })
            }
        }
    })
    .await
    .map_err(|e| format!("轮询任务异常: {e}"))?
}

// ============================================================================
// v0.3.16+：多账号管理命令
// ============================================================================

/// 列出所有账号（不含 PAT 本体）。
#[allow(dead_code)]
#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::list_accounts(&conn)
}

/// 添加账号：构造客户端探测真实 login（验证 PAT 有效），再写入 DB。
/// 若探测到的真实 login 与用户输入的 login 不一致，**仍接受**——PAT 的真实 login
/// 是权威；但同时把探测到的 login 回写到 DB 字段，确保 sync 时不会用错。
#[allow(dead_code)]
#[tauri::command]
pub fn add_account(
    state: State<'_, AppState>,
    label: String,
    login: String,
    org: String,
    pat: String,
) -> Result<Account, String> {
    let pat = pat.trim().to_string();
    if pat.is_empty() {
        return Err("PAT 不能为空".to_string());
    }
    // 锁外探测（避免阻塞 db 锁做网络 IO）；构造 + 探测真实 login。
    let probe_login = crate::github::GitHubClient::new(pat.clone(), String::new(), String::new())
        .and_then(|c| c.test_connection())
        .map(|r| r.login)
        .map_err(|e| format!("PAT 验证失败: {}", e))?;
    // 用探测到的真实 login 作权威；用户输入的 login 仅作 hint。
    let final_login = if probe_login.is_empty() {
        login.trim().to_string()
    } else {
        probe_login
    };
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let id = crate::db::insert_account(&conn, &label, &final_login, &org, &pat)?;
    // 若这是首个账号，把 active_account_id 拨到它。
    let active: i64 = crate::db::get_setting(&conn, "active_account_id")
        .parse()
        .unwrap_or(0);
    if active == 0 {
        crate::db::set_setting(&conn, "active_account_id", &id.to_string())?;
    }
    // 清掉旧的同步错误——这次同步才用到新账号的 PAT。
    crate::db::set_setting(&conn, "last_sync_error", "")?;
    let accounts = crate::db::list_accounts(&conn)?;
    accounts
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| format!("账号 #{id} 创建后未找到"))
}

/// 更新账号字段；pat=None 表示不动，pat=Some("") 表示清空，pat=Some(s) 表示替换。
/// 若更新了 PAT，先在锁外探测验证，避免坏 token 入库。
#[allow(dead_code)]
#[tauri::command]
pub fn update_account(
    state: State<'_, AppState>,
    id: i64,
    label: Option<String>,
    login: Option<String>,
    org: Option<String>,
    pat: Option<String>,
) -> Result<Account, String> {
    if let Some(p) = pat.as_ref() {
        let p = p.trim();
        if !p.is_empty() {
            // 锁外探测（构造 + test_connection 校验 PAT 有效）
            let probe = crate::github::GitHubClient::new(p.to_string(), String::new(), String::new())
                .and_then(|c| c.test_connection())
                .map_err(|e| format!("新 PAT 验证失败: {}", e))?;
            let _ = probe;
        }
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::update_account(
        &conn,
        id,
        label.as_deref(),
        login.as_deref(),
        org.as_deref(),
        pat.as_deref(),
    )?;
    let accounts = crate::db::list_accounts(&conn)?;
    accounts
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| format!("账号 #{id} 不存在"))
}

/// 删除账号；默认账号不可删。
#[allow(dead_code)]
#[tauri::command]
pub fn delete_account(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::delete_account(&conn, id)?;
    // 若删的是激活账号，回退到默认账号或首个账号。
    let active: i64 = crate::db::get_setting(&conn, "active_account_id")
        .parse()
        .unwrap_or(0);
    if active == id {
        if let Ok(new_id) = crate::db::default_account_id(&conn) {
            crate::db::set_setting(&conn, "active_account_id", &new_id.to_string())?;
        } else {
            crate::db::set_setting(&conn, "active_account_id", "0")?;
        }
    }
    Ok(())
}

/// 测试某账号的 PAT 是否仍有效；返回账号信息。
#[allow(dead_code)]
#[tauri::command]
pub fn test_account_pat(
    state: State<'_, AppState>,
    id: i64,
) -> Result<PatStatus, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let (login, org, pat) = crate::db::get_account_pat(&conn, id)?;
    drop(conn);
    if pat.is_empty() {
        return Err(format!("账号 #{id} ({login} / {org}) 未配置 PAT"));
    }
    let client = crate::github::GitHubClient::new(pat, String::new(), String::new())?;
    let probe = client.test_connection()?.login;
    Ok(PatStatus { login: probe, has_pat: true })
}

/// 把某账号设为默认；同时激活它。
#[allow(dead_code)]
#[tauri::command]
pub fn set_default_account(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::set_default_account(&conn, id)?;
    crate::db::set_setting(&conn, "active_account_id", &id.to_string())?;
    Ok(())
}

/// 设置激活账号 id；后续同步与默认视图都基于它。
#[allow(dead_code)]
#[tauri::command]
pub fn set_active_account(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    // 校验账号存在
    let exists: i64 = conn
        .query_row("SELECT COUNT(*) FROM accounts WHERE id = ?1", [id], |r| r.get(0))
        .unwrap_or(0);
    if exists == 0 {
        return Err(format!("账号 #{id} 不存在"));
    }
    crate::db::set_setting(&conn, "active_account_id", &id.to_string())?;
    Ok(())
}

/// 设置视图模式：'single' / 'all'。
#[allow(dead_code)]
#[tauri::command]
pub fn set_view_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    if mode != "single" && mode != "all" {
        return Err(format!("非法视图模式: {mode}（应为 single / all）"));
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::set_setting(&conn, "view_mode", &mode)?;
    Ok(())
}

// ============================================================================
// v0.3.19+：关于页面 —— 当前版本号 + 检查更新（GitHub Releases）
// ============================================================================

/// 返回当前应用版本号。来源为 Rust 包版本（Cargo.toml `version`），
/// 而非前端硬编码——保证「关于」页展示的版本与发布版本号一致。
#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 「检查更新」返回信息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckUpdate {
    /// 当前版本（Cargo 包版本）。
    pub current: String,
    /// GitHub 最新 release 的版本号（去掉前缀 `v`）。
    pub latest: String,
    /// 当前是否已是最新。
    pub up_to_date: bool,
    /// 最新 release 页面地址，用于引导跳转下载。
    pub url: String,
    /// 非空表示检查失败（网络 / 解析等），前端据此展示错误。
    pub error: String,
}

/// 轻量检查更新：调用 GitHub Releases API `releases/latest`，对比最新/当前版本。
/// 只读公开数据仓库，无需 PAT；用 `spawn_blocking` 避免阻塞主线程（reqwest 为 blocking）。
#[tauri::command]
pub async fn check_latest_release() -> Result<CheckUpdate, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let current = env!("CARGO_PKG_VERSION").to_string();
        let client = reqwest::blocking::Client::builder()
            .user_agent(format!("taskboard/{current}"))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| format!("构造 HTTP 客户端失败: {e}"))?;
        let resp: serde_json::Value = client
            .get("https://api.github.com/repos/ShawnLiuSZ/task-dashborad/releases/latest")
            .send()
            .map_err(|e| format!("检查更新失败（网络）：{e}"))?
            .error_for_status()
            .map_err(|e| format!("检查更新失败（GitHub 返回错误）：{e}"))?
            .json()
            .map_err(|e| format!("检查更新失败（解析响应）：{e}"))?;
        let latest = resp
            .get("tag_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string();
        let url = resp
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://github.com/ShawnLiuSZ/task-dashborad/releases")
            .to_string();
        let up_to_date = !latest.is_empty() && latest == current;
        Ok(CheckUpdate {
            current,
            latest,
            up_to_date,
            url,
            error: String::new(),
        })
    })
    .await
    .map_err(|e| format!("检查更新线程异常: {e}"))?
}
