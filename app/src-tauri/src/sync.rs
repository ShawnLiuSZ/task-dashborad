use rusqlite::Connection;
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::github;

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 归属判定：必须在结果里读 assignees 数组自行判断。
/// GitHub Search 不支持 `-no:assignee`（否定 no: 限定符会被静默忽略），无法用查询语法区分。
pub fn classify(assignees: &[String], login: &str) -> &'static str {
    if assignees.is_empty() {
        "notassignee"
    } else if assignees.iter().any(|a| a == login) {
        "assigned"
    } else {
        "assigned-others"
    }
}

/// 将 GitHub Project（OMS Kanban）的 Status 字段原文映射到看板四态。
/// 用关键词匹配以兼容 emoji / 文案微调；返回 None 表示无法识别（回落到本地手动态）。
///
/// 映射依据 OMS Kanban 实际可选值（经用户确认，v0.3.6 起沿用，未做改动）：
/// 🧠需求池 / 🤔产品规划 / 🚧待开发处理 → 待处理
/// ✨开发中 → 处理中
/// 🔎开发完成/测试中 / ✅测试通过/待上线 → 已处理
/// 🎉完成/上线 / ↩️取消 → 已完成
fn map_project_status(raw: &str) -> Option<&'static str> {
    if raw.contains("测试") {
        Some("processed")
    } else if raw.contains("开发中") {
        Some("doing")
    } else if raw.contains("待开发") || raw.contains("需求") || raw.contains("规划") {
        Some("todo")
    } else if raw.contains("取消") {
        Some("done")
    } else if raw.contains("完成") || raw.contains("上线") {
        Some("done")
    } else {
        None
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncResult {
    pub total: usize,
    pub added: usize,
    pub updated: usize,
    pub candidate_done: usize,
    pub removed: usize,
    pub pruned: usize,
    pub warning: String,
    pub synced_at: i64,
}

/// 从文本（PR 正文）里提取 issue 引用，返回 `repo#number` 形式的键。
/// 支持 `#123`（归属 PR 所在仓库）与 `owner/repo#123`（跨仓库）；无任何第三方依赖地手动扫描。
/// 这是把「PR 对应哪个 issue」反向关联回看板的依据。
fn parse_issue_refs(text: &str, default_repo: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            let num_start = i + 1;
            let mut j = num_start;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if let Ok(num) = text[num_start..j].parse::<i64>() {
                if num > 0 {
                    // '#' 前若形如 owner/repo（含 '/')，则用其中仓库名；否则用 PR 所在仓库。
                    let mut k = i as isize - 1;
                    while k >= 0
                        && (bytes[k as usize].is_ascii_alphanumeric()
                            || bytes[k as usize] == b'_'
                            || bytes[k as usize] == b'-'
                            || bytes[k as usize] == b'.')
                    {
                        k -= 1;
                    }
                    let seg = &text[(k + 1) as usize..i];
                    let repo = seg.rfind('/').map(|s| &seg[s + 1..]).unwrap_or(default_repo);
                    if !repo.is_empty() {
                        out.push(format!("{}#{}", repo, num));
                    }
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

pub fn run(conn: &Connection) -> Result<SyncResult, String> {
    let org = crate::db::get_setting(conn, "org");
    let pat = crate::db::get_setting(conn, "pat_token");
    if pat.is_empty() {
        // 不算失败（用户只是还没填 PAT），由 lib.rs 显示提示横幅并跳过本次同步。
        return Err("未配置 GitHub PAT，请在设置面板粘贴 token（fine-grained 推荐）".to_string());
    }
    let client = github::GitHubClient::new(pat)?;
    let login = client.login().to_string();

    // 多源合并：以多个稳定查询（assignee/author/mentions/commenter）覆盖 `involves:`
    // 的偶发漏拉缺陷，确保任何「与我相关」的 issue 都不会缺失。按 key 去重。
    // best-effort：单源失败不中断整次同步，其余源照常并入（避免一次接口抖动让看板整体失效）。
    let sources: Vec<(&str, Result<Vec<github::RawTask>, String>)> = vec![
        ("assignee", client.fetch_assigned(&org)),
        ("author", client.fetch_authored(&org)),
        ("mentions", client.fetch_mentioned(&org)),
        ("commenter", client.fetch_commented(&org)),
        ("involves", client.fetch_related(&org)),
    ];
    let mut lists: Vec<Vec<github::RawTask>> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut mention_keys: HashSet<String> = HashSet::new();
    let mut mention_ok = false;
    for (name, res) in sources {
        match res {
            Ok(tasks) => {
                if name == "mentions" {
                    for t in &tasks {
                        mention_keys.insert(format!("{}#{}", t.repo, t.number));
                    }
                    mention_ok = true;
                }
                lists.push(tasks);
            }
            Err(e) => {
                failed.push(format!("{}: {}", name, e));
                eprintln!("[sync] 数据源 {} 拉取失败，已跳过: {}", name, e);
            }
        }
    }
    if lists.is_empty() {
        return Err(format!("全部数据源拉取失败: {}", failed.join("; ")));
    }
    let raw = github::merge_tasks_all(lists);
    let now = now_secs();

    // 收集「与我相关 issue 实际所在的仓库」（去重），仅对这些仓库拉取 PR——
    // 既缩小范围、又避开 Search API 的严苛限流，改用 REST pulls 接口。
    let mut pr_repos: Vec<String> = raw
        .iter()
        .filter(|t| !t.is_pr)
        .map(|t| t.repo.clone())
        .collect();
    pr_repos.sort();
    pr_repos.dedup();

    // PR 拉取走核心配额（5000/h）不需二次冷却；Search 调用间的 1s 间隔由
    // GitHubClient.search 内部保证。
    let (prs, pr_fetch_ok) = match client.fetch_prs(&org, &pr_repos) {
        Ok(p) => (p, true),
        Err(e) => {
            eprintln!("[sync] 拉取 PR 列表失败，跳过 PR 关联: {}", e);
            (Vec::new(), false)
        }
    };
    let mut pr_map: std::collections::HashMap<String, (i64, String, String)> = std::collections::HashMap::new();
    for pr in &prs {
        // 同一 issue 可能被多个 PR 引用，取第一个命中的 PR 即可（卡片只需一个链接 + 分支）。
        for rk in parse_issue_refs(&pr.body, &pr.repo) {
            pr_map
                .entry(rk)
                .or_insert((pr.number, pr.url.clone(), pr.head_ref.clone()));
        }
    }
    // 诊断：记录 PR 关联的拉取与命中情况（便于排查关联为空的问题，可保留无害）。
    let _ = crate::db::set_setting(conn, "diag_pr_fetch_ok", &pr_fetch_ok.to_string());
    let _ = crate::db::set_setting(conn, "diag_pr_fetched", &prs.len().to_string());
    let _ = crate::db::set_setting(conn, "diag_pr_map_size", &pr_map.len().to_string());
    let _ = crate::db::set_setting(conn, "diag_pr_matched", &raw.iter().filter(|t| !t.is_pr && pr_map.contains_key(&format!("{}#{}", t.repo, t.number))).count().to_string());

    // 拉取 OMS Kanban 项目 Status 字段（best-effort）：失败时地图为空，
    // 看板对这些 issue 退化为"维持本地手动态"，不影响其余同步。
    let project_status = match client.fetch_project_status(&org) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[sync] 拉取项目状态失败，跳过状态联动: {}", e);
            std::collections::HashMap::new()
        }
    };

    conn.execute("UPDATE tasks SET stale = 1", [])
        .map_err(|e| format!("标记陈旧任务失败: {}", e))?;

    let mut added = 0usize;
    let mut updated = 0usize;
    // 单次同步内回源拉取评论的预算（控制 API 调用量，避免突发限流）；其余 issue 顺延到后续同步补。
    // 由 30 降至 12：评论回源是同步末段最重的增量调用，缩减可显著降低总调用量、给二次限流窗口留余量；
    // 且 PR 关联（核心特性）已在评论之前完成，即便评论预算耗尽也不影响 PR 链路。
    let mut comment_budget: usize = 12;
    for t in &raw {
        if t.is_pr {
            continue;
        }
        let key = format!("{}#{}", t.repo, t.number);
        let ownership: &str = classify(&t.assignees, &login);

        // 读取既有状态与富化字段缓存：用于"不在项目中"时维持本地手动态；新任务默认待处理。
        // comments/mentioned/pr/branch 等上次同步的缓存值一并读出，本次增量更新失败时保留既有值。
        let row: (String, i64, i64, i64, String, String, String) = conn
            .query_row(
                "SELECT status, comments_count, mentioned, pr_number, pr_url, latest_comment_url, branch FROM tasks WHERE key = ?1",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .unwrap_or_else(|_| ("todo".to_string(), 0, 0, 0, String::new(), String::new(), String::new()));
        let existing_status = row.0;
        let existing_comments = row.1;
        let existing_mentioned = row.2;
        let existing_pr_number = row.3;
        let existing_pr_url = row.4;
        let existing_comment_url = row.5;
        let existing_branch = row.6;
        let exists = conn
            .query_row("SELECT 1 FROM tasks WHERE key = ?1", [&key], |_| Ok(()))
            .is_ok();

        // 决定看板状态：closed→已完成（v0.3.5）；项目中→按 Project Status 映射（覆盖本地）；
        // 不在项目中→维持本地手动态。gh_status 记录原始文案，供前端展示与追溯。
        let gh_status_raw = project_status.get(&key).cloned().unwrap_or_default();
        let final_status: &str = if t.state == "closed" {
            "done"
        } else if !gh_status_raw.is_empty() {
            map_project_status(&gh_status_raw).unwrap_or(&existing_status)
        } else {
            &existing_status
        };

        let assignees_csv = t.assignees.join(",");
        // done_at 记录「进入已完成」的时刻：首次变为 done 时打上 now，之后保持不变，
        // 以便按 30 天窗口清理；移出 done 时归零（重做会重新计时）。
        let done_at_val = if final_status == "done" { now } else { 0 };

        // 评论区 @我：以 GitHub mentions 搜索结果为准（覆盖正文与评论中的 @），
        // 分配给我时不重复提示。mentions 源整体失败时保留既有标记。
        let mentioned = if mention_ok {
            mention_keys.contains(&key) && ownership != "assigned"
        } else {
            existing_mentioned != 0
        };
        let mentioned_val: i64 = if mentioned { 1 } else { 0 };

        // PR 关联：仅在 PR 列表拉取成功时更新（失败则保留既有值，避免误清空）。
        // branch 同样来自关联 PR 的 head.ref——issue 无分支字段，只能从 PR 反向取。
        let (pr_number, pr_url, branch): (i64, String, String) = if pr_fetch_ok {
            match pr_map.get(&key) {
                Some((n, u, b)) => (*n, u.clone(), b.clone()),
                None => (0, String::new(), String::new()),
            }
        } else {
            (existing_pr_number, existing_pr_url, existing_branch)
        };

        // 新评论链接：仅当评论数较上次增加且预算充足时回源拉取（控制 API 调用量），
        // 取最新一条评论的永久链接；其余情况沿用缓存。
        let (comments_count, latest_comment_url): (i64, String) =
            if t.comments > existing_comments as u64 && comment_budget > 0 {
                match client.fetch_comments(&org, &t.repo, t.number) {
                    Ok(Some(url)) => {
                        comment_budget -= 1;
                        std::thread::sleep(Duration::from_millis(80));
                        (t.comments as i64, url)
                    }
                    Ok(None) => {
                        comment_budget -= 1;
                        (t.comments as i64, existing_comment_url)
                    }
                    Err(e) => {
                        eprintln!("[sync] 拉取评论失败，跳过: {}#{}: {}", t.repo, t.number, e);
                        (existing_comments, existing_comment_url)
                    }
                }
            } else {
                (existing_comments, existing_comment_url)
            };

        conn.execute(
            "INSERT INTO tasks
               (key, owner, repo, number, title, url, gh_state, ownership,
                status, gh_status, assignees, done_at, mentioned, comments_count,
                latest_comment_url, pr_number, pr_url, branch, candidate_done, stale, updated_at, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 0, 0, ?19, ?20)
             ON CONFLICT(key) DO UPDATE SET
               title = excluded.title,
               repo = excluded.repo,
               gh_state = excluded.gh_state,
               ownership = excluded.ownership,
               updated_at = excluded.updated_at,
               synced_at = excluded.synced_at,
               candidate_done = 0,
               stale = 0,
               gh_status = excluded.gh_status,
               assignees = excluded.assignees,
               status = excluded.status,
               done_at = CASE
                 WHEN excluded.status = 'done' AND done_at = 0 THEN ?20
                 WHEN excluded.status <> 'done' THEN 0
                 ELSE done_at
               END,
               mentioned = excluded.mentioned,
               comments_count = excluded.comments_count,
               latest_comment_url = excluded.latest_comment_url,
               pr_number = excluded.pr_number,
               pr_url = excluded.pr_url,
               branch = excluded.branch",
            rusqlite::params![
                key,
                org,
                t.repo,
                t.number,
                t.title,
                t.url,
                t.state,
                ownership,
                final_status,
                gh_status_raw,
                assignees_csv,
                done_at_val,
                mentioned_val,
                comments_count,
                latest_comment_url,
                pr_number,
                pr_url,
                branch,
                t.updated_at,
                now,
            ],
        )
        .map_err(|e| format!("写入任务失败: {}", e))?;

        if exists {
            updated += 1;
        } else {
            added += 1;
        }
    }

    // 处理本次未出现的任务：关闭的标为候选已完成，仍打开但已不相关的移出看板。
    let mut stale_rows: Vec<(String, String, i64)> = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT key, repo, number FROM tasks WHERE stale = 1")
            .map_err(|e| format!("查询陈旧任务失败: {}", e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            })
            .map_err(|e| format!("遍历陈旧任务失败: {}", e))?;
        for r in rows {
            stale_rows.push(r.map_err(|e| e.to_string())?);
        }
    }

    let mut candidate_done = 0usize;
    let mut removed = 0usize;
    for (key, repo, number) in stale_rows {
        match client.fetch_state(&org, &repo, number) {
            Ok(state) if state == "closed" => {
                // GitHub 已关闭：状态最终态为「已完成」——以远程真实状态为准，
                // 覆盖本地手动态（即便曾被标为处理中/已处理，关闭即视为做完）。
                // candidate_done 保留作「远程已关闭、待本地确认归档」的提示。
                // done_at：首次进入已完成时打上 now（done_at 原为空），之后保持不变。
                conn.execute(
                    "UPDATE tasks SET candidate_done = 1, gh_state = 'closed', status = 'done', stale = 0, done_at = CASE WHEN done_at = 0 THEN ?2 ELSE done_at END WHERE key = ?1",
                    rusqlite::params![&key, &now],
                )
                .map_err(|e| format!("标记候选已完成失败: {}", e))?;
                candidate_done += 1;
            }
            Ok(_) => {
                // 仍打开，但已不在搜索结果中（如 assignee 变更、不再 involves 我）：移出看板。
                conn.execute("DELETE FROM tasks WHERE key = ?1", [&key])
                    .map_err(|e| format!("移除失效任务失败: {}", e))?;
                removed += 1;
            }
            Err(_) => {
                // 查询失败（限流 / 网络抖动 / 超时）：**保留任务**，等下次同步再判定，
                // 绝不在不确定时删除，避免一次限流误清空整个看板。
                eprintln!("[sync] fetch_state 失败，保留任务不过删: {}", key);
            }
        }
    }

    // 已完成任务保留 1 个月：超过 30 天的自动清理，保持看板清爽。
    // done_at = 0 表示「完成时间未知」（v0.3.8 前的历史数据），为安全起见不清理，
    // 仅对带真实完成时间戳的新任务按窗口淘汰。
    let pruned = conn
        .execute(
            "DELETE FROM tasks WHERE status = 'done' AND done_at > 0 AND ?1 - done_at > 2592000",
            [now],
        )
        .map_err(|e| format!("清理过期已完成任务失败: {}", e))?;

    crate::db::set_setting(conn, "last_sync_at", &now.to_string())?;

    let total: usize = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap_or(0);

    Ok(SyncResult {
        total,
        added,
        updated,
        candidate_done,
        removed,
        pruned,
        warning: if failed.is_empty() {
            String::new()
        } else {
            format!("部分数据源拉取失败，已用其余源同步: {}", failed.join("; "))
        },
        synced_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::Connection;

    /// 头less 全量同步验证：直接打开生产库（与应用共用同一 SQLite 文件），
    /// 跑一次真实 `run`，再核对「PR 关联」是否真正落地（pr_number > 0 的任务数 > 0）。
    ///
    /// 这是本会话修复（PR 拉取 60s 超时 + 阶段冷却 + 评论预算缩减）的唯一端到端验收手段——
    /// 逻辑层已用 Python 镜像证明 parse_issue_refs + 真实 PR 正文可命中 ~44/77 任务，
    /// 但 PR 拉取能否在 GitHub 二次限流下成功，只能靠真实同步验证。
    ///
    /// 依赖真实 GitHub 网络与限流配额，默认忽略，需时以
    /// `cargo test --lib -- --ignored test_headless_sync_pr_linkage` 显式运行。
    /// 会就地改写生产库（与应用每次同步一致），运行前请确认已备份。
    #[test]
    #[ignore]
    fn test_headless_sync_pr_linkage() {
        let prod = std::path::Path::new(
            "/Users/liushizhao/Library/Application Support/com.liushizhao.taskboard/taskboard.db",
        );
        assert!(
            prod.exists(),
            "生产库应已存在（先运行过一次 App 同步）"
        );
        // 关键：复制到临时库再跑，**绝不改写用户的生产库**。
        // 早期版本直接开生产库跑同步，属于误改用户数据的高危写法——任何 `cargo test --lib -- --ignored`
        // 都会触发，故改为临时副本，验证后删除。
        let tmp = std::env::temp_dir().join("taskboard_headless_test.db");
        std::fs::copy(prod, &tmp).expect("复制生产库到临时文件");
        eprintln!("[test] 已复制生产库到临时文件（不影响生产数据）：{}", tmp.display());
        let conn = Connection::open(&tmp).expect("打开临时库");
        // 临时副本沿用生产库 schema；补上本次新增列，确保 run() 里引用 branch 的 UPSERT 不报错。
        let _ = conn.execute("ALTER TABLE tasks ADD COLUMN branch TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE tasks ADD COLUMN handoff TEXT NOT NULL DEFAULT ''", []);

        eprintln!("[test] 开始真实同步（关注 PR 关联）…");
        let res = run(&conn).expect("同步应成功");
        eprintln!(
            "[test] 同步完成：total={} added={} updated={} removed={} candidate_done={} pruned={}",
            res.total, res.added, res.updated, res.removed, res.candidate_done, res.pruned
        );

        let pr_gt0: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks WHERE pr_number > 0", [], |r| r.get(0))
            .unwrap();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        let diag_fetch_ok = db::get_setting(&conn, "diag_pr_fetch_ok");
        let diag_fetched = db::get_setting(&conn, "diag_pr_fetched");
        let diag_map = db::get_setting(&conn, "diag_pr_map_size");
        let diag_matched = db::get_setting(&conn, "diag_pr_matched");
        eprintln!(
            "[test] PR 关联命中：{}/{}  | diag_fetch_ok={} diag_fetched={} diag_map_size={} diag_matched={}",
            pr_gt0, total, diag_fetch_ok, diag_fetched, diag_map, diag_matched
        );
        let _ = std::fs::remove_file(&tmp);
        assert!(pr_gt0 > 0, "期望至少 1 个任务的 pr_number > 0（PR 关联应落地）");
    }
}
