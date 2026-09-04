use rusqlite::{Connection, Result};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// 应用标识符，与 tauri.conf.json 的 `identifier` 一致。
/// 同时用于推导无 GUI 运行时的本地数据目录（MCP 子命令等）。
pub const APP_IDENTIFIER: &str = "com.liushizhao.taskboard";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tasks (
  key            TEXT PRIMARY KEY,
  owner          TEXT NOT NULL,
  repo           TEXT NOT NULL,
  number         INTEGER NOT NULL,
  title          TEXT NOT NULL,
  url            TEXT NOT NULL,
  gh_state       TEXT NOT NULL,
  ownership      TEXT NOT NULL,
  status         TEXT NOT NULL DEFAULT 'todo',
  session_id     TEXT,
  session_agent  TEXT,
  session_at     INTEGER,
  candidate_done INTEGER NOT NULL DEFAULT 0,
  stale          INTEGER NOT NULL DEFAULT 0,
  gh_status      TEXT NOT NULL DEFAULT '',
  assignees      TEXT NOT NULL DEFAULT '',
  done_at        INTEGER NOT NULL DEFAULT 0,
  mentioned      INTEGER NOT NULL DEFAULT 0,
  comments_count INTEGER NOT NULL DEFAULT 0,
  latest_comment_url TEXT NOT NULL DEFAULT '',
  pr_number      INTEGER NOT NULL DEFAULT 0,
  pr_url         TEXT NOT NULL DEFAULT '',
  updated_at     TEXT,
  synced_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_ownership ON tasks(ownership);

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("schedule_minutes", "60"),
    ("gh_path", ""),
    ("login", ""),
    ("org", "FoodsUp-Inc"),
    // v0.3.15：GitHub Personal Access Token。完全替换 gh CLI 路径，
    // 由设置面板（SettingsPanel）粘入后写入；为空时同步跳过并提示。
    ("pat_token", ""),
    // v0.3.15：最近一次同步的错误信息（PAT 为空 / API 错误等），前端可读此字段显示横幅。
    ("last_sync_error", ""),
];

pub fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建数据目录失败: {}", e))?;
    Ok(dir.join("taskboard.db"))
}

/// 无 AppHandle 时的数据目录（与 Tauri `app_data_dir` 解析一致）：
/// `~/Library/Application Support/com.liushizhao.taskboard`
pub fn data_dir() -> Result<PathBuf, String> {
    let base = dirs::data_dir().ok_or_else(|| "无法定位用户数据目录".to_string())?;
    Ok(base.join(APP_IDENTIFIER))
}

/// MCP 子命令等无 GUI 运行时使用的默认库路径；可用 `TASKBOARD_DB` 环境变量覆盖。
pub fn db_path_default() -> Result<PathBuf, String> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建数据目录失败: {}", e))?;
    Ok(dir.join("taskboard.db"))
}

/// 打开（必要时创建）数据库连接，应用 schema 与历史迁移，并写入默认设置。
/// GUI 与 MCP 子命令共用此函数，确保表结构单一来源、无漂移。
pub fn open_db(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("打开数据库失败: {}", e))?;
    conn.execute_batch(SCHEMA)
        .map_err(|e| format!("初始化表结构失败: {}", e))?;
    // 迁移：兼容已存在的旧库，缺列则补（列已存在时 ALTER 会报错，忽略即可）。
    for col_sql in [
        "ALTER TABLE tasks ADD COLUMN gh_status TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE tasks ADD COLUMN assignees TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE tasks ADD COLUMN done_at INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE tasks ADD COLUMN mentioned INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE tasks ADD COLUMN comments_count INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE tasks ADD COLUMN latest_comment_url TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE tasks ADD COLUMN pr_number INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE tasks ADD COLUMN pr_url TEXT NOT NULL DEFAULT ''",
        // v0.3.10：关联 PR 的分支（head.ref），以及 agent 写入的交接任务详情。
        "ALTER TABLE tasks ADD COLUMN branch TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE tasks ADD COLUMN handoff TEXT NOT NULL DEFAULT ''",
    ] {
        let _ = conn.execute(col_sql, []);
    }
    for (k, v) in DEFAULT_SETTINGS {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO NOTHING",
            rusqlite::params![k, v],
        )
        .map_err(|e| format!("写入默认设置失败: {}", e))?;
    }
    Ok(conn)
}

pub fn init(app: &AppHandle) -> Result<Connection, String> {
    open_db(&db_path(app)?)
}

pub fn get_setting(conn: &Connection, key: &str) -> String {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .unwrap_or_default()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(|e| format!("保存设置失败: {}", e))?;
    Ok(())
}
