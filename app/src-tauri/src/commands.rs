use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

fn rows_to_tasks(conn: &Connection, ownership: Option<&str>) -> Result<Vec<Task>, String> {
    let (sql, use_filter) = match ownership {
        Some(_) => (
            "SELECT key, owner, repo, number, title, url, gh_state, ownership, status, gh_status,
                    assignees, mentioned, latest_comment_url, pr_number, pr_url, branch,
                    session_id, session_agent, session_at, candidate_done, handoff, updated_at
             FROM tasks WHERE ownership = ?1
             ORDER BY candidate_done ASC, status ASC, updated_at DESC",
            true,
        ),
        None => (
            "SELECT key, owner, repo, number, title, url, gh_state, ownership, status, gh_status,
                    assignees, mentioned, latest_comment_url, pr_number, pr_url, branch,
                    session_id, session_agent, session_at, candidate_done, handoff, updated_at
             FROM tasks
             ORDER BY candidate_done ASC, status ASC, updated_at DESC",
            false,
        ),
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
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
        })
    };

    let rows = match use_filter {
        true => stmt.query_map([ownership.unwrap_or("")], mapper),
        false => stmt.query_map([], mapper),
    };
    let mut out = Vec::new();
    for r in rows.map_err(|e| e.to_string())? {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn list_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    ownership: Option<String>,
) -> Result<Vec<Task>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let _ = app;
    rows_to_tasks(&conn, ownership.as_deref())
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
    let db_path = crate::db::db_path(&app)
        .map_err(|e| e.to_string())?;
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
    // 先在锁外构造客户端（避免构造时阻塞 db 锁）。
    let client = crate::github::GitHubClient::new(trimmed.clone())
        .map_err(|e| format!("PAT 验证失败: {}", e))?;
    let login = client.login().to_string();
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
    // 取锁外构造（reqwest 的网络 IO 与 db 锁解耦）。
    drop(conn);
    let client = crate::github::GitHubClient::new(pat)?;
    Ok(PatStatus { login: client.login().to_string(), has_pat: true })
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
