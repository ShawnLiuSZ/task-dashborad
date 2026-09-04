//! 端到端集成测试：db 模块 + v0.3.16 多账号迁移。
//!
//! 不依赖网络（纯本地 SQLite），CI 可直接跑。
//! 全部测试都先在临时目录开一份独立 SQLite，
//! 避免污染 `~/Library/Application Support/com.liushizhao.taskboard/taskboard.db`。

use rusqlite::Connection;
use std::sync::atomic::{AtomicU64, Ordering};
use taskboard_lib::db as db;

/// 计数器 + pid + nanos 保证并发测试之间不会撞同一个 tempdir。
/// 之前用纯 nanos 在多线程并行时被撞到，导致 accounts 表被前一个测试写过。
static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> Connection {
    let dir = tempdir();
    let path = dir.join("taskboard.db");
    db::open_db(&path).expect("open_db 必须成功")
}

/// 临时目录（不依赖 tempfile crate，零外部依赖）。
fn tempdir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pid = std::process::id();
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "taskboard_test_{}_{}_{}",
        pid, nanos, n
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ===== Schema / 基础读写 ============================================

#[test]
fn open_db_creates_schema_and_writes_defaults() {
    let conn = fresh_db();
    // 大部分 DEFAULT_SETTINGS 都应有值；`gh_path` / `pat_token` 之类 v0.3.15 起
    // 仅作兼容兜底，默认空字符串是合法状态。
    for (k, v_default) in db::DEFAULT_SETTINGS.iter() {
        let actual = db::get_setting(&conn, k);
        if *v_default == "" {
            continue; // 允许为空
        }
        assert!(!actual.is_empty(), "默认设置 {} 不应为空", k);
    }
    // 任务表可写可读。
    conn.execute(
        "INSERT INTO tasks (key, owner, repo, number, title, url, gh_state,
                            ownership, status, synced_at, account_id)
         VALUES ('r#1','o','r',1,'t','u','open','assigned','todo',1,1)",
        [],
    )
    .expect("INSERT 任务必须成功");
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn set_setting_is_idempotent_and_overwrites() {
    let conn = fresh_db();
    db::set_setting(&conn, "schedule_minutes", "15").unwrap();
    assert_eq!(db::get_setting(&conn, "schedule_minutes"), "15");
    db::set_setting(&conn, "schedule_minutes", "30").unwrap();
    assert_eq!(db::get_setting(&conn, "schedule_minutes"), "30");
}

// ===== accounts CRUD ================================================

#[test]
fn list_accounts_initially_empty() {
    let conn = fresh_db();
    let accs = db::list_accounts(&conn).unwrap();
    assert!(accs.is_empty(), "全新库不应有 accounts（迁移前）");
}

#[test]
fn insert_first_account_becomes_default() {
    let conn = fresh_db();
    let id = db::insert_account(&conn, "工作", "alice", "FoodsUp-Inc", "ghp_xxx").unwrap();
    let accs = db::list_accounts(&conn).unwrap();
    assert_eq!(accs.len(), 1);
    assert_eq!(accs[0].id, id);
    assert!(accs[0].is_default);
    assert_eq!(accs[0].login, "alice");
    assert!(accs[0].has_pat, "PAT 已配，has_pat 应为 true");
}

#[test]
fn insert_second_account_is_not_default() {
    let conn = fresh_db();
    let _ = db::insert_account(&conn, "工作", "alice", "FoodsUp-Inc", "ghp_a").unwrap();
    let id2 = db::insert_account(&conn, "个人", "bob", "other-org", "ghp_b").unwrap();
    let accs = db::list_accounts(&conn).unwrap();
    assert_eq!(accs.len(), 2);
    let second = accs.iter().find(|a| a.id == id2).unwrap();
    assert!(!second.is_default);
    assert_eq!(second.label, "个人");
}

#[test]
fn insert_account_validates_required_fields() {
    let conn = fresh_db();
    assert!(db::insert_account(&conn, "", "alice", "o", "p").is_err());
    assert!(db::insert_account(&conn, "L", "", "o", "p").is_err());
    assert!(db::insert_account(&conn, "L", "alice", "", "p").is_err());
    assert!(db::insert_account(&conn, "L", "alice", "o", "").is_err());
    // 全部合法
    assert!(db::insert_account(&conn, "  L  ", "  alice  ", "  o  ", "  p  ").is_ok());
    let accs = db::list_accounts(&conn).unwrap();
    // trim 必须生效
    assert_eq!(accs[0].label, "L");
    assert_eq!(accs[0].login, "alice");
    assert_eq!(accs[0].org, "o");
}

#[test]
fn update_account_partial_and_full() {
    let conn = fresh_db();
    let id = db::insert_account(&conn, "L", "alice", "FoodsUp", "ghp_old").unwrap();
    // 只改 label
    db::update_account(&conn, id, Some("新名字"), None, None, None).unwrap();
    let accs = db::list_accounts(&conn).unwrap();
    assert_eq!(accs[0].label, "新名字");
    assert_eq!(accs[0].login, "alice");

    // pat=Some("") 表示清空
    db::update_account(&conn, id, None, None, None, Some("")).unwrap();
    let accs = db::list_accounts(&conn).unwrap();
    assert!(!accs[0].has_pat);

    // pat=Some(s) 替换
    db::update_account(&conn, id, None, None, None, Some("ghp_new")).unwrap();
    let accs = db::list_accounts(&conn).unwrap();
    assert!(accs[0].has_pat);

    // 不存在的 id
    assert!(db::update_account(&conn, 999, Some("x"), None, None, None).is_err());

    // 空 label/login/org 触发校验
    assert!(db::update_account(&conn, id, Some(""), None, None, None).is_err());
    assert!(db::update_account(&conn, id, None, Some(""), None, None).is_err());
    assert!(db::update_account(&conn, id, None, None, Some(""), None).is_err());

    // pat=None 不动
    db::update_account(&conn, id, None, None, None, None).unwrap();
    let accs = db::list_accounts(&conn).unwrap();
    assert!(accs[0].has_pat, "pat=None 不应影响");
}

#[test]
fn set_default_account_swaps_flag() {
    let conn = fresh_db();
    let a = db::insert_account(&conn, "A", "alice", "FoodsUp", "ghp_a").unwrap();
    let b = db::insert_account(&conn, "B", "bob", "FoodsUp", "ghp_b").unwrap();
    assert_eq!(db::default_account_id(&conn).unwrap(), a);
    db::set_default_account(&conn, b).unwrap();
    assert_eq!(db::default_account_id(&conn).unwrap(), b);
    // A 不再默认
    let accs = db::list_accounts(&conn).unwrap();
    let a_rec = accs.iter().find(|x| x.id == a).unwrap();
    assert!(!a_rec.is_default);
    let b_rec = accs.iter().find(|x| x.id == b).unwrap();
    assert!(b_rec.is_default);
    // 不存在的 id 报错
    assert!(db::set_default_account(&conn, 999).is_err());
}

#[test]
fn delete_account_blocks_default_and_orphan_task_id_kept() {
    let conn = fresh_db();
    let a = db::insert_account(&conn, "A", "alice", "FoodsUp", "ghp_a").unwrap();
    let b = db::insert_account(&conn, "B", "bob", "FoodsUp", "ghp_b").unwrap();
    // 默认账号不可删
    assert!(db::delete_account(&conn, a).is_err());
    // 切换默认后，A 可删
    db::set_default_account(&conn, b).unwrap();
    assert!(db::delete_account(&conn, a).is_ok());
    let accs = db::list_accounts(&conn).unwrap();
    assert_eq!(accs.len(), 1);
    assert_eq!(accs[0].id, b);
    // 删不存在的 id
    assert!(db::delete_account(&conn, 999).is_err());

    // 把账号删后，旧任务的 account_id 不应被联动清理（保留作 UI 上的「账号已删除」徽章）。
    // 这条用例：插一条任务归属被删除的账号 a，然后删另一个账号 b，验证 b 的删除不影响 a 的任务归属。
    let c = db::insert_account(&conn, "C", "carol", "FoodsUp", "ghp_c").unwrap();
    // 插一条归属 a（已删）的任务——account_id=1 是历史值
    conn.execute(
        "INSERT INTO tasks (key, owner, repo, number, title, url, gh_state,
                            ownership, status, synced_at, account_id)
         VALUES ('r#1','o','r',1,'t','u','open','assigned','todo',1,?1)",
        [a],
    )
    .unwrap();
    // 再插一条归属 b 的任务，删 b 后这条也应保留
    conn.execute(
        "INSERT INTO tasks (key, owner, repo, number, title, url, gh_state,
                            ownership, status, synced_at, account_id)
         VALUES ('r#2','o','r',2,'t2','u2','open','assigned','todo',1,?1)",
        [b],
    )
    .unwrap();
    // C 设为默认
    db::set_default_account(&conn, c).unwrap();
    // b 不是默认，可删
    assert!(db::delete_account(&conn, b).is_ok());
    let still_a: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE account_id = ?1",
            [a],
            |r| r.get(0),
        )
        .unwrap();
    let still_b: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE account_id = ?1",
            [b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        still_a, 1,
        "归属已删账号的 task.account_id 必须保留，便于 UI 标注账号已删除"
    );
    assert_eq!(
        still_b, 1,
        "归属当前被删账号的 task.account_id 同样保留（孤儿任务）
         ——不在 delete_account 路径上清除，因为任务本身可能还在被代理领着"
    );
}

#[test]
fn get_account_pat_returns_full_tuple() {
    let conn = fresh_db();
    let id = db::insert_account(&conn, "L", "alice", "FoodsUp", "ghp_secret").unwrap();
    let (login, org, pat) = db::get_account_pat(&conn, id).unwrap();
    assert_eq!(login, "alice");
    assert_eq!(org, "FoodsUp");
    assert_eq!(pat, "ghp_secret");
    assert!(db::get_account_pat(&conn, 999).is_err());
}

// ===== v0.3.15 → v0.3.16 迁移 =======================================

#[test]
fn migrate_v0315_to_accounts_inserts_default_when_meta_has_pat() {
    let dir = tempdir();
    let path = dir.join("taskboard.db");

    // 第一阶段：模拟「v0.3.15 老库」—— 直接写 pat_token / login / org，
    //             不建立 accounts 表，等下次 open_db 时迁移。
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
           key TEXT PRIMARY KEY, value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS tasks (
           key TEXT PRIMARY KEY, owner TEXT, repo TEXT, number INTEGER,
           title TEXT, url TEXT, gh_state TEXT, ownership TEXT,
           status TEXT, session_id TEXT, session_agent TEXT, session_at INTEGER,
           candidate_done INTEGER DEFAULT 0, stale INTEGER DEFAULT 0,
           gh_status TEXT DEFAULT '', assignees TEXT DEFAULT '',
           done_at INTEGER DEFAULT 0, mentioned INTEGER DEFAULT 0,
           comments_count INTEGER DEFAULT 0, latest_comment_url TEXT DEFAULT '',
           pr_number INTEGER DEFAULT 0, pr_url TEXT DEFAULT '',
           updated_at TEXT,
           synced_at INTEGER,
           account_id INTEGER NOT NULL DEFAULT 1
         );",
    )
    .unwrap();
    db::set_setting(&conn, "pat_token", "ghp_legacy").unwrap();
    db::set_setting(&conn, "login", "alice").unwrap();
    db::set_setting(&conn, "org", "FoodsUp-Inc").unwrap();
    // 插一条旧任务（account_id 默认 1 —— 我们要让迁移后改到真实的新账号 id）
    conn.execute(
        "INSERT INTO tasks (key, owner, repo, number, title, url, gh_state,
                            ownership, status, synced_at)
         VALUES ('r#1','o','r',1,'t','u','open','assigned','todo',1)",
        [],
    )
    .unwrap();
    drop(conn);

    // 第二阶段：用 open_db 触发迁移。
    let conn = db::open_db(&path).unwrap();
    let accs = db::list_accounts(&conn).unwrap();
    assert_eq!(accs.len(), 1, "应迁出一条默认账号");
    assert!(accs[0].is_default);
    assert_eq!(accs[0].login, "alice");
    assert_eq!(accs[0].org, "FoodsUp-Inc");
    assert!(accs[0].has_pat);

    let active: i64 = db::get_setting(&conn, "active_account_id").parse().unwrap();
    assert_eq!(active, accs[0].id, "active_account_id 应指向新账号 id");

    // 旧任务的 account_id 应被调整到新 id，避免指向一个不存在的账号 1
    let new_id = accs[0].id;
    let updated: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE account_id = ?1",
            [new_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(updated, 1, "旧任务的 account_id 必须重写到新账号 id");
}

#[test]
fn migrate_v0315_skips_when_no_legacy_pat() {
    // 新用户从 v0.3.16 直接开：accounts 空 + meta.pat_token 为空 → 不迁。
    let conn = fresh_db();
    let accs = db::list_accounts(&conn).unwrap();
    assert!(accs.is_empty(), "全新用户不应自动产生账号");
}

#[test]
fn migrate_v0315_idempotent_when_accounts_exists() {
    let conn = fresh_db();
    // 模拟「已迁过」：用户先加了一条账号，再有人写 meta.pat_token。
    let _ = db::insert_account(&conn, "L", "alice", "FoodsUp", "ghp_x").unwrap();
    db::set_setting(&conn, "pat_token", "ghp_legacy").unwrap();
    let accs = db::list_accounts(&conn).unwrap();
    assert_eq!(accs.len(), 1, "不应重复迁移");
}

// ===== WAL + journal_mode 持久化 ====================================

#[test]
fn open_db_uses_wal_journal_mode() {
    // v0.3.16+：open_db 强制 WAL + NORMAL sync，避免 DELETE journal 半提交
    // 导致下次启动 disk I/O error。系统 sqlite3 CLI 看到 journal_mode=wal 即合规。
    let conn = fresh_db();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    // 注：journal_mode 在同一连接上是持久的；跨新连接 SQLite 仍可能回退到 DELETE
    // 直到 checkpoint 完成。这里只断言"open_db 后立刻查就是 WAL"。
    assert_eq!(mode.to_lowercase(), "wal");
}

#[test]
fn open_db_recovers_from_dirty_journal_file() {
    // v0.3.16 之前：DELETE journal 半提交 → 下次启动 SQLite 报 disk I/O error 直至崩溃。
    // WAL 模式 + 自动 forward-rollback 不再有此问题；本用例制造 .db-journal 残留，
    // 确认 open_db 仍能正常返回。
    let dir = tempdir();
    let path = dir.join("taskboard.db");
    // 第一次 open_db：建表，写默认设置。
    let _ = db::open_db(&path).expect("first open_db OK");
    // 手工伪造一个 dirty journal 残留（模拟之前 DELETE 模式下的崩溃场景）。
    std::fs::write(
        path.with_extension("db-journal"),
        b"\x00\x00\x00\x00garbage journal bytes",
    )
    .unwrap();
    // 二次 open_db：必须能成功（即使有 journal 残留）。
    let conn = db::open_db(&path).expect("open_db 应能容忍残留 journal");
    // 表应仍可读可写。
    let n: i64 = conn
        .query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    conn.execute(
        "INSERT INTO accounts (label, login, org, pat_token, is_default, created_at)
         VALUES ('L','alice','o','p',1,1)",
        [],
    )
    .unwrap();
}
