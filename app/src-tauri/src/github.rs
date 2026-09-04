use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const JQ_ITEMS: &str = r#"[.items[] | {
  number, title, url: .html_url, state, updated_at,
  repo: (.repository_url | split("/") | .[-1]),
  assignees: [.assignees[].login],
  comments: (.comments // 0),
  is_pr: (.pull_request != null)
}]"#;

#[derive(Debug, Deserialize, Clone)]
pub struct RawTask {
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub updated_at: String,
    pub repo: String,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub comments: u64,
    #[serde(default)]
    pub is_pr: bool,
}

/// 一个 PR 的精简信息，用于把「issue 对应的 PR」关联回看板卡片。
#[derive(Debug, Deserialize, Clone)]
pub struct RawPr {
    // 注意：REST pulls 的 JQ 投影（JQ_PRS_REST）不输出 repo 字段，repo 由 fetch_prs 调用方按当前仓库回填。
    // 故此处必须 `#[serde(default)]`，否则反序列化会因「缺少 repo 字段」而失败（先前 flutter-driver 即因此报错）。
    #[serde(default)]
    pub repo: String,
    pub number: i64,
    pub url: String,
    #[serde(default)]
    pub body: String,
    // PR 所在分支（head.ref）。issue 没有分支字段，只能从关联 PR 取；缺失时回落空串。
    #[serde(default)]
    pub head_ref: String,
}

/// PR（REST pulls）结果投影：REST 接口的数组直接是 PR 列表（无 .items 包裹），
/// 仓库名由调用方按当前仓库传入。正文用于解析其引用的 issue 编号；
/// head.ref 是该 PR 的分支名（issue 本身没有分支字段，只能从关联 PR 反向取）。
const JQ_PRS_REST: &str = r#"[.[] | {
  number, url: .html_url, body: (.body // ""), head_ref: (.head.ref // "")
}]"#;

#[derive(Debug, Deserialize)]
struct IssueState {
    state: String,
}

/// 探测 gh 可执行文件位置：优先用户配置，其次常见安装路径，最后回落到 PATH。
pub fn resolve_gh(cfg: &str) -> Result<String, String> {
    if !cfg.is_empty() {
        if Path::new(cfg).exists() {
            return Ok(cfg.to_string());
        }
        return Err(format!("配置的 gh 路径不存在: {}", cfg));
    }
    for p in [
        "/opt/homebrew/bin/gh",
        "/usr/local/bin/gh",
        "/opt/local/bin/gh",
    ] {
        if Path::new(p).exists() {
            return Ok(p.to_string());
        }
    }
    let out = Command::new("sh")
        .args(["-lc", "command -v gh"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    out.ok_or_else(|| "未找到 gh，请在设置中指定其路径".to_string())
}

/// 单次 gh 调用超时（秒）。GitHub Search API 限流时 gh 会退避重试并长时间挂起，
/// 若不设上限，一次卡住的 gh 会让整个同步永久阻塞。
const GH_TIMEOUT_SECS: u64 = 25;

/// 判断错误是否疑似 GitHub 限流（Search API 尤其严格，偶发 429/403 会让 gh 快速失败并带明确错误）。
/// 这类错误值得短歇后重试。注意：**超时（gh 长时间挂起）不算限流**——超时是连接/服务器卡死，
/// 重试几乎必然再次超时，只会把单次失败从 35s 放大成 35s×重试次数，拖垮整次同步，故超时不重试。
fn is_rate_limited(err: &str) -> bool {
    err.contains("rate limit")
        || err.contains("429")
        || err.contains("403")
        || err.contains("Retry-After")
        || err.contains("abuse")
        || err.contains("secondary")
}

/// 单次 gh 调用（带超时保护），超时秒数由调用方指定。超时或非零退出都返回 Err，由外层决定是否重试。
///
/// 为什么超时参数要外置：GitHub 限流时 `gh` 会**自行**按 `Retry-After` 退避重试，等待时长
/// 可能长达数十秒。PR 拉取这类关键路径若用 25s 超时，会把「gh 正在等比限流恢复」误判为
/// 「卡死」而草率杀掉、导致整次 PR 关联清零；故 PR 拉取放宽到 60s，给 gh 的内部退避留足空间。
/// 普通搜索仍用默认 25s——搜索失败只是单源降级，代价小。
fn run_gh_once(gh: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    run_gh_once_timed(gh, args, GH_TIMEOUT_SECS)
}

fn run_gh_once_timed(gh: &str, args: &[&str], timeout_secs: u64) -> Result<Vec<u8>, String> {
    // 调用间小歇，避免一次同步里 5 个 issue 搜索 + 多仓库 PR 拉取形成突发请求，
    // 触发 GitHub 的二次（突发）限流——那会让 gh 退避重试并长时间挂起、最终拉取超时，
    // 把整次 PR 关联清零。800ms 的间隔把请求速率压到限流阈值以下，且单次同步仅多花十余秒。
    thread::sleep(Duration::from_millis(800));
    let mut child = Command::new(gh)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 gh 失败: {}", e))?;

    // 关键：必须用独立线程**并发**排空 stdout / stderr。
    // 否则当 gh 输出超过 OS 管道缓冲（macOS 约 64KB，例如 fad-backend 单页 PR JSON 达 442KB）时，
    // gh 写满管道后被阻塞、进程无法退出，本函数只能等到超时再 kill——这就是此前「PR 拉取 60s 超时、
    // 仅小仓库 flutter-driver 成功、pr_number 恒为 0」的真正根因（与 GitHub 限流无关）。
    // 并发排空后 gh 不会阻塞，正常 1~3s 即返回。
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法获取 gh stdout 句柄".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法获取 gh stderr 句柄".to_string())?;
    let out_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut buf);
        buf
    });
    let err_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stderr, &mut buf);
        buf
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| format!("等待 gh 失败: {}", e))? {
            Some(s) => break s,
            None => {
                if start.elapsed() > Duration::from_secs(timeout_secs) {
                    let _ = child.kill();
                    return Err(format!("gh 调用超时（>{}s）: {}", timeout_secs, args.join(" ")));
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
    };

    let out = out_thread.join().unwrap_or_default();
    let err = err_thread.join().unwrap_or_default();

    if !status.success() {
        return Err(format!(
            "gh 调用失败: {}",
            String::from_utf8_lossy(&err).trim()
        ));
    }
    Ok(out)
}

/// 带限流重试的 gh 调用：疑似限流（429/403/超时）时短歇后重试最多 2 次；
/// 其余错误立即返回。超过重试仍失败的，由调用方的 best-effort 逻辑降级处理。
fn run_gh(gh: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut last_err = String::new();
    for attempt in 0..3 {
        match run_gh_once(gh, args) {
            Ok(out) => return Ok(out),
            Err(e) => {
                last_err = e.clone();
                if attempt < 2 && is_rate_limited(&e) {
                    eprintln!("[gh] 疑似限流，{}s 后重试（第 {} 次）: {}", 3, attempt + 1, e);
                    thread::sleep(Duration::from_secs(3));
                    continue;
                }
                return Err(e);
            }
        }
    }
    Err(last_err)
}

pub fn current_login(gh: &str) -> Result<String, String> {
    let out = run_gh(gh, &["api", "user", "--jq", ".login"])?;
    let login = String::from_utf8_lossy(&out).trim().to_string();
    if login.is_empty() {
        return Err("无法获取 GitHub 账号，请先执行 gh auth login".to_string());
    }
    Ok(login)
}

/// 通用搜索执行：统一处理分页上限与 JSON 解析。
fn fetch_search(gh: &str, q: &str) -> Result<Vec<RawTask>, String> {
    let out = run_gh(
        gh,
        &[
            "api",
            "-X",
            "GET",
            "search/issues",
            "-f",
            &format!("q={}", q),
            "-f",
            "per_page=100",
            "--jq",
            JQ_ITEMS,
        ],
    )?;
    serde_json::from_slice::<Vec<RawTask>>(&out)
        .map_err(|e| format!("解析 GitHub 返回失败: {}", e))
}

/// 拉取「与我相关」的全部 open issue（author/assignee/mention/commenter 任意命中）。
/// 仅作补充源：GitHub 的 `involves:` 对 assignee 覆盖偶发不可靠，故主覆盖由下方专属查询保证。
pub fn fetch_related(gh: &str, org: &str, login: &str) -> Result<Vec<RawTask>, String> {
    fetch_search(gh, &format!("org:{} involves:{} is:open is:issue", org, login))
}

/// 拉取「明确分配给我」的 open issue。权威来源，确保「分配给我」永不漏拉。
pub fn fetch_assigned(gh: &str, org: &str, login: &str) -> Result<Vec<RawTask>, String> {
    fetch_search(gh, &format!("org:{} assignee:{} is:open is:issue", org, login))
}

/// 拉取「我创建」的 open issue。
pub fn fetch_authored(gh: &str, org: &str, login: &str) -> Result<Vec<RawTask>, String> {
    fetch_search(gh, &format!("org:{} author:{} is:open is:issue", org, login))
}

/// 拉取「@提到我」的 open issue。
pub fn fetch_mentioned(gh: &str, org: &str, login: &str) -> Result<Vec<RawTask>, String> {
    fetch_search(gh, &format!("org:{} mentions:{} is:open is:issue", org, login))
}

/// 拉取「我评论过」的 open issue。
pub fn fetch_commented(gh: &str, org: &str, login: &str) -> Result<Vec<RawTask>, String> {
    fetch_search(gh, &format!("org:{} commenter:{} is:open is:issue", org, login))
}

/// 合并多组搜索结果，按 `repo#number` 去重（后写入者覆盖前者）。
pub fn merge_tasks_all(lists: Vec<Vec<RawTask>>) -> Vec<RawTask> {
    let mut map: std::collections::HashMap<String, RawTask> = std::collections::HashMap::new();
    for list in lists {
        for t in list {
            map.insert(format!("{}#{}", t.repo, t.number), t);
        }
    }
    map.into_values().collect()
}

/// 拉取「与我相关 issue 所在仓库」的全部 PR（open + closed），用于把「issue 对应的 PR」
/// 关联回看板卡片。
///
/// 关键设计：改用 REST `repos/{org}/{repo}/pulls` 接口（而非 Search API 的 `is:pr`），
/// 因为 Search API 限流极严（认证后约 30 次/分钟），一次同步里 5 个 issue 搜索 + 多页 PR
/// 搜索极易打满，导致 `gh` 退避重试并长时间挂起、最终拉取超时。REST pulls 走核心配额
/// （5000 次/小时），无此瓶颈；且只拉「用户 issue 实际所在的少数仓库」，请求量本就很小。
/// **逐页 best-effort + 重试**：单页失败不结清次关联，仍失败则跳过该仓库继续下一个。
pub fn fetch_prs(gh: &str, org: &str, repos: &[String]) -> Result<Vec<RawPr>, String> {
    let mut all: Vec<RawPr> = Vec::new();
    for repo in repos {
        if repo.is_empty() {
            continue;
        }
        for page in 1..=3 {
            // 单页单发、单次尝试：限流/抖动交给 `gh` 内部退避重试（它会按 Retry-After 自动等），
            // 我们只在「gh 卡死 >60s」时才放弃该页——宁可让 gh 等比限流恢复成功、也不草率清零。
            // 不用 run_gh 的 3× 重试：那会把单次 60s 超时放大成 180s/页，拖垮整次同步。
            // 查询参数必须拼在 URL 里，不可用 `-f` 传入——`gh api pulls -f state=all` 会被误判为
            // 「创建 PR」要求 base/head 返回 422。
            let url = format!(
                "repos/{}/{}/pulls?state=all&per_page=100&page={}",
                org, repo, page
            );
            let page_items: Vec<RawPr> = match run_gh_once_timed(gh, &["api", &url, "--jq", JQ_PRS_REST], 60) {
                Ok(o) => match serde_json::from_slice::<Vec<RawPr>>(&o) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("[sync] {}/PR 第 {} 页解析失败，跳过: {}", repo, page, e);
                        Vec::new()
                    }
                },
                Err(e) => {
                    let _ = std::fs::write(
                        "/tmp/taskboard_pr_err.log",
                        format!("{}/PR 第 {} 页拉取失败: {}", repo, page, e),
                    );
                    eprintln!("[sync] {}/PR 第 {} 页拉取失败，跳过: {}", repo, page, e);
                    Vec::new()
                }
            };
            let n = page_items.len();
            for mut pr in page_items {
                pr.repo = repo.clone();
                all.push(pr);
            }
            if n < 100 {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
    Ok(all)
}

/// 拉取某 issue 的全部评论，返回最新一条评论的永久链接（html_url），供卡片一键跳转。
/// 评论按创建时间升序返回，取末位即最新；无评论返回 None。best-effort：失败返回 Err。
pub fn fetch_comments(
    gh: &str,
    owner: &str,
    repo: &str,
    number: i64,
) -> Result<Option<String>, String> {
    let path = format!(
        "repos/{}/{}/issues/{}/comments?per_page=100",
        owner, repo, number
    );
    let out = run_gh(gh, &["api", &path, "--jq", "[.[].html_url]"])?;
    let urls: Vec<String> = serde_json::from_slice(&out)
        .map_err(|e| format!("解析评论失败: {}", e))?;
    Ok(urls.into_iter().last())
}

/// 查询单个 issue 的当前状态，用于判定「已从搜索结果中消失」的任务是关闭还是不再相关。
pub fn fetch_state(gh: &str, owner: &str, repo: &str, number: i64) -> Result<String, String> {
    let path = format!("repos/{}/{}/issues/{}", owner, repo, number);
    let out = run_gh(gh, &["api", &path, "--jq", "{state}"])?;
    let parsed: IssueState = serde_json::from_slice(&out)
        .map_err(|e| format!("解析 issue 状态失败: {}", e))?;
    Ok(parsed.state)
}

/// 执行一条 GraphQL 查询：写入临时文件后用 `gh api graphql -F query=@<file>` 调用，
/// 返回解析后的 JSON。复用 run_gh 的超时机制，避免接口挂起拖垮整次同步。
fn run_gh_graphql(gh: &str, query: &str) -> Result<serde_json::Value, String> {
    let tmp = std::env::temp_dir().join(format!("taskboard_gql_{}.graphql", std::process::id()));
    std::fs::write(&tmp, query).map_err(|e| format!("写 GraphQL 临时文件失败: {}", e))?;
    let args = ["api", "graphql", "-F", &format!("query=@{}", tmp.display())];
    let out = run_gh(gh, &args);
    let _ = std::fs::remove_file(&tmp);
    let out = out?;
    serde_json::from_slice(&out).map_err(|e| format!("解析 GraphQL 返回失败: {}", e))
}

/// 拉取 GitHub Project「OMS Kanban」中每个 issue 的 Status 字段，
/// 返回 `repo#number -> Status 原文` 的映射，供 sync 映射到看板四态。
///
/// 为什么需要它：看板状态联动不能只看 issue 的 open/closed——团队用 Project 的
/// Status 字段（如「🔎开发完成/测试中」）表达进度，而 Search API 不返回该字段。
/// 一次分页拉全项目条目（含关联 issue 的仓库与编号），比逐 issue 查询高效得多。
pub fn fetch_project_status(gh: &str, org: &str) -> Result<HashMap<String, String>, String> {
    // 1. 找到 OMS Kanban 项目的 id（项目归属组织，按标题匹配）。
    let find_q = format!(
        r#"query {{ organization(login:"{org}") {{ projectsV2(first:50) {{ nodes {{ id title }} }} }} }}"#
    );
    let find = run_gh_graphql(gh, &find_q)?;
    let nodes = find["data"]["organization"]["projectsV2"]["nodes"]
        .as_array()
        .ok_or_else(|| "无法读取组织项目列表".to_string())?;
    let project_id = nodes
        .iter()
        .find(|n| n["title"].as_str() == Some("OMS Kanban"))
        .and_then(|n| n["id"].as_str())
        .ok_or_else(|| "未找到 OMS Kanban 项目（请确认项目名或 token 具备 project 读权限）".to_string())?
        .to_string();

    // 2. 分页拉取项目全部条目，构建 `repo#number -> Status`。
    let mut map: HashMap<String, String> = HashMap::new();
    let mut cursor: Option<String> = None;
    for _ in 0..100 {
        let after = match &cursor {
            Some(c) => format!(r#", after:"{}""#, c),
            None => String::new(),
        };
        let items_q = format!(
            r#"query {{ node(id:"{pid}") {{ ... on ProjectV2 {{ items(first:50{after}) {{
              pageInfo {{ hasNextPage endCursor }}
              nodes {{
                content {{ ... on Issue {{ number repository {{ name }} }} }}
                fieldValues(first:20) {{
                  nodes {{ ... on ProjectV2ItemFieldSingleSelectValue {{
                    name field {{ ... on ProjectV2SingleSelectField {{ name }} }}
                  }} }}
                }}
              }}
            }} }} }} }}"#,
            pid = project_id,
            after = after
        );
        let resp = run_gh_graphql(gh, &items_q)?;
        let items = &resp["data"]["node"]["items"];
        let page_nodes = items["nodes"]
            .as_array()
            .ok_or_else(|| "项目条目格式异常".to_string())?;
        for n in page_nodes {
            let content = &n["content"];
            let num = content["number"].as_i64();
            let repo = content["repository"]["name"].as_str();
            if let (Some(num), Some(repo)) = (num, repo) {
                let key = format!("{}#{}", repo, num);
                let mut status = String::new();
                if let Some(fvs) = n["fieldValues"]["nodes"].as_array() {
                    for fv in fvs {
                        if fv["field"]["name"].as_str() == Some("Status") {
                            if let Some(name) = fv["name"].as_str() {
                                status = name.to_string();
                            }
                        }
                    }
                }
                if !status.is_empty() {
                    map.insert(key, status);
                }
            }
        }
        if items["pageInfo"]["hasNextPage"].as_bool() == Some(true) {
            cursor = items["pageInfo"]["endCursor"].as_str().map(|s| s.to_string());
        } else {
            break;
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 fetch_prs（REST pulls 路径）能真正拉到 PR 并解析正文。
    /// 仅依赖 REST 接口（核心配额 5000/hr），不受 Search API 限流影响，
    /// 因此即使搜索接口被限流也能独立验证 PR 关联链路。
    /// 注：依赖真实 GitHub 网络与限流配额，默认忽略，需时以
    /// `cargo test --lib -- --ignored test_fetch_prs_rest_returns_data` 显式运行。
    #[test]
    #[ignore]
    fn test_fetch_prs_rest_returns_data() {
        let gh = resolve_gh("").expect("gh 应可用");
        let org = std::env::var("TASKBOARD_TEST_ORG").unwrap_or_else(|_| "FoodsUp-Inc".to_string());
        let repos = vec![
            "fad-backend".to_string(),
            "pq-backend".to_string(),
            "flutter-driver".to_string(),
        ];
        let prs = fetch_prs(&gh, &org, &repos).expect("fetch_prs 不应报错");
        eprintln!("[test] fetched {} PRs across test repos", prs.len());
        assert!(!prs.is_empty(), "应至少拉到一个 PR");
        // 抽查：每个 PR 应有编号、链接，且 repo 字段被回填。
        for pr in prs.iter().take(3) {
            assert!(pr.number > 0);
            assert!(pr.url.contains("github.com"));
            assert!(!pr.repo.is_empty());
        }
    }
}
