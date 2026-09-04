//! GitHub 客户端（v0.3.15+）：完全替换 gh CLI 路径，改为 PAT + reqwest 直接调 REST/GraphQL。
//!
//! 历史背景：v0.3.15 之前所有同步逻辑都通过 `gh` 子进程拉数据，由此引入了一连串历史包袱
//! （探测 gh 路径、子进程 stdout/stderr 管道阻塞、`gh api graphql -F` 的临时文件、
//! `involves:` 偶发漏拉、`assignee: listed_user` 触发 Search API 422 等）。本次彻底移除 gh，
//! 改由本模块封装的 [`GitHubClient`] 用 GitHub Personal Access Token 直接调官方 REST/GraphQL，
//! 同步行为完全可控、与系统 `gh auth switch` 解耦。
//!
//! 限流策略：主动解析 `X-RateLimit-Remaining` / `X-RateLimit-Reset` 与 `Retry-After` 头部，
//! 触发阈值前主动 sleep；Search API 调用间固定 1s 间隔（认证后 30 req/min 上限）。
//! 重试失败由调用方（sync.rs）的 best-effort 合并逻辑降级。

use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Search API 调用的固定间隔（毫秒）。GitHub Search API 认证后严格 30 req/min，
/// 折合 1 次/2s；为应对突发限流计数窗口的余量抖动，用 1s 间隔保守调度。
const SEARCH_INTERVAL_MS: u64 = 1000;

/// 单次请求主动 sleep 上限（毫秒）。某些场景下 `Retry-After` 可能给出极大值，
/// 这里限制上限以免一次同步被挂死——超出后直接放弃本次调用。
const MAX_BACKOFF_MS: u64 = 30_000;

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
    /// REST pulls 数组本身不输出 repo 字段，由调用方按当前仓库回填。
    #[serde(default)]
    pub repo: String,
    pub number: i64,
    pub url: String,
    #[serde(default)]
    pub body: String,
    /// PR 所在分支（head.ref）；issue 没有分支字段，只能从关联 PR 反向取。
    #[serde(default)]
    pub head_ref: String,
}

/// 仅在 Search API 路径作为分页上限 fallback 使用；当前实现改为请求 `per_page=100`
/// 后整页解析，不再依赖 JQ 投影。
#[allow(dead_code)]
const SEARCH_PROJECTION: &str = "[.items[] | {
  number, title, url: .html_url, state, updated_at,
  repo: (.repository_url | split(\"/\") | .[-1]),
  assignees: [.assignees[].login],
  comments: (.comments // 0),
  is_pr: (.pull_request != null)
}]";

/// REST pulls 投影同样改为原生解析；保留常量仅为在调试中对照使用。
#[allow(dead_code)]
const PRS_REST_PROJECTION: &str = "[.[] | {
  number, url: .html_url, body: (.body // \"\"), head_ref: (.head.ref // \"\")
}]";

/// `search/issues` 走 Search API（严格限流 30 req/min），其余走核心配额（5000/h）。
/// 调用方需用不同入口区分，避免 Search API 计数污染核心配额统计。
const SEARCH_PATH: &str = "search/issues";

/// GitHub 客户端：一次构造长期复用（共享 reqwest 连接池）。
///
/// 构造时会调一次 `user` API 探测并缓存 `login`——这是同步逻辑（authored / assignee /
/// mentions / commenter）所必须。空 PAT 构造直接返回错误，由调用方提示用户去设置面板补 token。
pub struct GitHubClient {
    pat: String,
    login: String,
    http: reqwest::blocking::Client,
}

impl GitHubClient {
    /// 构造客户端：探测 GitHub 账号，把 login 缓存到本对象。
    ///
    /// 成功：返回可用客户端。
    /// 失败：PAT 缺失 / 鉴权失败 / 网络错误——错误信息可定向给最终用户。
    pub fn new(pat: String) -> Result<Self, String> {
        let pat = pat.trim().to_string();
        if pat.is_empty() {
            return Err("GitHub PAT 为空，请在设置面板粘贴 token".to_string());
        }
        let http = reqwest::blocking::Client::builder()
            .user_agent("taskboard/0.3.15")
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("构造 HTTP 客户端失败: {}", e))?;
        let probe = Self { pat: pat.clone(), login: String::new(), http };
        let login = probe.test_connection()?.login;
        Ok(Self { pat, login, http: probe.http })
    }

    /// 探测当前 PAT 是否有效，并返回账号登录名。
    /// 专供设置面板「测试连接」按钮调用；构造时也复用。
    pub fn test_connection(&self) -> Result<TestConnectionResult, String> {
        let url = "https://api.github.com/user";
        let resp = self.get(url)?;
        let login = resp
            .get("login")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "GitHub 返回无 login 字段".to_string())?
            .to_string();
        if login.is_empty() {
            return Err("GitHub 返回空 login（账号不可用）".to_string());
        }
        Ok(TestConnectionResult { login })
    }

    pub fn login(&self) -> &str {
        &self.login
    }

    /// 拉取「明确分配给我」的 open issue。权威来源，确保「分配给我」永不漏拉。
    pub fn fetch_assigned(&self, org: &str) -> Result<Vec<RawTask>, String> {
        self.search(&format!("org:{} assignee:{} is:open is:issue", org, self.login))
    }

    /// 拉取「我创建」的 open issue。
    pub fn fetch_authored(&self, org: &str) -> Result<Vec<RawTask>, String> {
        self.search(&format!("org:{} author:{} is:open is:issue", org, self.login))
    }

    /// 拉取「@提到我」的 open issue。
    pub fn fetch_mentioned(&self, org: &str) -> Result<Vec<RawTask>, String> {
        self.search(&format!("org:{} mentions:{} is:open is:issue", org, self.login))
    }

    /// 拉取「我评论过」的 open issue。
    pub fn fetch_commented(&self, org: &str) -> Result<Vec<RawTask>, String> {
        self.search(&format!("org:{} commenter:{} is:open is:issue", org, self.login))
    }

    /// 拉取「与我相关」的全部 open issue。仅作补充源：GitHub 的 `involves:` 对
    /// assignee 覆盖偶发不可靠，故主覆盖由上面 4 个专属查询保证。
    pub fn fetch_related(&self, org: &str) -> Result<Vec<RawTask>, String> {
        self.search(&format!("org:{} involves:{} is:open is:issue", org, self.login))
    }

    /// 拉取指定仓库的全部 PR（open + closed），用于把「PR 关联的 issue」反向关联回看板卡片。
    ///
    /// REST pulls 接口（核心配额 5000/h，无 Search API 的 30/min 严限）。
    /// 逐页 best-effort：单页失败仅记录日志跳过，不中断整个仓库列表。
    pub fn fetch_prs(&self, org: &str, repos: &[String]) -> Result<Vec<RawPr>, String> {
        let mut all: Vec<RawPr> = Vec::new();
        for repo in repos {
            if repo.is_empty() {
                continue;
            }
            for page in 1..=3 {
                let url = format!(
                    "https://api.github.com/repos/{}/{}/pulls?state=all&per_page=100&page={}",
                    org, repo, page
                );
                // 走核心配额，不计入 Search API 节流；且 PR 数据量可能很大（一次同步达数十 MB），
                // 设较大超时避免大仓库拉取被中断。
                let items: Vec<RawPr> = match self.get_with_timeout(&url, 60) {
                    Ok(v) => match serde_json::from_value::<Vec<RawPr>>(v) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("[sync] {}/PR 第 {} 页解析失败，跳过: {}", repo, page, e);
                            Vec::new()
                        }
                    },
                    Err(e) => {
                        eprintln!("[sync] {}/PR 第 {} 页拉取失败，跳过: {}", repo, page, e);
                        Vec::new()
                    }
                };
                let n = items.len();
                for mut pr in items {
                    pr.repo = repo.clone();
                    all.push(pr);
                }
                if n < 100 {
                    break;
                }
            }
        }
        Ok(all)
    }

    /// 拉取某 issue 的全部评论，返回最新一条评论的永久链接（html_url），供卡片一键跳转。
    /// 无评论返回 None。best-effort：失败返回 Err。
    pub fn fetch_comments(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<Option<String>, String> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/issues/{}/comments?per_page=100",
            owner, repo, number
        );
        let v = self.get(&url)?;
        let arr = v.as_array().ok_or_else(|| "comments 返回非数组".to_string())?;
        Ok(arr
            .iter()
            .filter_map(|c| c.get("html_url").and_then(|u| u.as_str()).map(String::from))
            .last())
    }

    /// 查询单个 issue 的当前状态（open/closed），用于「陈旧任务」回路判定。
    pub fn fetch_state(&self, owner: &str, repo: &str, number: i64) -> Result<String, String> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/issues/{}",
            owner, repo, number
        );
        let v = self.get(&url)?;
        v.get("state")
            .and_then(|s| s.as_str())
            .map(String::from)
            .ok_or_else(|| "issue 响应无 state 字段".to_string())
    }

    /// 拉取 GitHub Project「OMS Kanban」中每个 issue 的 Status 字段，
    /// 返回 `repo#number -> Status 原文` 的映射，供 sync 映射到看板四态。
    ///
    /// 为什么需要：看板状态联动不能只看 issue 的 open/closed——团队用 Project 的
    /// Status 字段（如「🔎开发完成/测试中」）表达进度，Search API 不返回该字段。
    pub fn fetch_project_status(&self, org: &str) -> Result<HashMap<String, String>, String> {
        // 1. 找到 OMS Kanban 项目的 id（按标题匹配）。
        let find_q = format!(
            r#"query {{ organization(login:"{org}") {{ projectsV2(first:50) {{ nodes {{ id title }} }} }} }}"#
        );
        let find = self.graphql(&find_q)?;
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
            let resp = self.graphql(&items_q)?;
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
                cursor = items["pageInfo"]["endCursor"]
                    .as_str()
                    .map(|s| s.to_string());
            } else {
                break;
            }
        }
        Ok(map)
    }

    // ===== 私有方法 =====

    /// 限流感知的 GET：依据 `X-RateLimit-Remaining` / `X-RateLimit-Reset` / `Retry-After`
    /// 主动 sleep；遇 4xx/5xx 返回带状态码的错误。
    fn get(&self, url: &str) -> Result<serde_json::Value, String> {
        self.get_with_timeout(url, self.http_timeout())
    }

    fn get_with_timeout(&self, url: &str, timeout_secs: u64) -> Result<serde_json::Value, String> {
        // 主动节流：Search API 严格 30 req/min。其它路径虽然走核心配额，但仍尊重响应头的
        // 剩余计数，避免触发 Search API 二次（突发）限流。
        if url.contains(SEARCH_PATH) {
            std::thread::sleep(Duration::from_millis(SEARCH_INTERVAL_MS));
        }

        for attempt in 0..3 {
            let resp = self
                .http
                .get(url)
                .header("Authorization", format!("Bearer {}", self.pat))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .timeout(Duration::from_secs(timeout_secs))
                .send()
                .map_err(|e| format!("网络请求失败: {}", e))?;

            let status = resp.status();
            // 1. 主动限流：响应头 Retry-After 数值（秒）遵守。
            if status.as_u16() == 429 || status.as_u16() == 403 {
                let retry_after = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .or_else(|| self.seconds_until_reset(resp.headers()))
                    .unwrap_or(10);
                let wait_ms = (retry_after * 1000).min(MAX_BACKOFF_MS);
                eprintln!(
                    "[gh] 限流（{}），等待 {}ms 后重试（第 {} 次）",
                    status.as_u16(),
                    wait_ms,
                    attempt + 1
                );
                std::thread::sleep(Duration::from_millis(wait_ms));
                continue;
            }

            // 2. 其它非 2xx（如 404/422/401）：立即返回错误，由 best-effort 逻辑降级。
            if !status.is_success() {
                let body = resp.text().unwrap_or_default();
                return Err(format!(
                    "GitHub API 错误 ({}): {}",
                    status.as_u16(),
                    body.chars().take(160).collect::<String>()
                ));
            }

            // 3. 成功：根据 X-RateLimit-Remaining 决定是否需要主动 sleep 等配额回补。
            let remaining: Option<i64> = resp
                .headers()
                .get("X-RateLimit-Remaining")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok());
            if let Some(r) = remaining {
                if r <= 2 {
                    // 即将耗尽，主动等到窗口重置；上限保护避免挂死。
                    let wait_ms = self
                        .seconds_until_reset(resp.headers())
                        .map(|s| (s * 1000).min(MAX_BACKOFF_MS))
                        .unwrap_or(5000);
                    eprintln!(
                        "[gh] 配额剩余 {}，等待 {}ms 回补",
                        r, wait_ms
                    );
                    std::thread::sleep(Duration::from_millis(wait_ms));
                }
            }

            return resp
                .json::<serde_json::Value>()
                .map_err(|e| format!("解析 GitHub 返回失败: {}", e));
        }
        Err(format!("达到最大重试次数（限流持续）: {}", url))
    }

    /// Search API 调用的统一入口：单页结果（自动投影）。
    fn search(&self, q: &str) -> Result<Vec<RawTask>, String> {
        // search/issues?q=...&per_page=100
        let encoded = urlencode(q);
        let url = format!(
            "https://api.github.com/{}?q={}&per_page=100",
            SEARCH_PATH, encoded
        );
        let v = self.get(&url)?;
        let items = v
            .get("items")
            .and_then(|i| i.as_array())
            .ok_or_else(|| "search 响应无 items 数组".to_string())?;
        serde_json::from_value::<Vec<RawTask>>(serde_json::Value::Array(items.clone()))
            .map_err(|e| format!("解析 search 返回失败: {}", e))
    }

    /// GraphQL POST：把 query 直接放进 JSON body。
    fn graphql(&self, query: &str) -> Result<serde_json::Value, String> {
        let url = "https://api.github.com/graphql";
        let body = serde_json::json!({ "query": query });
        let resp = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {}", self.pat))
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .map_err(|e| format!("GraphQL 网络请求失败: {}", e))?;
        let status = resp.status();
        let v: serde_json::Value = resp
            .json()
            .map_err(|e| format!("解析 GraphQL 返回失败: {}", e))?;
        if !status.is_success() {
            return Err(format!(
                "GraphQL API 错误 ({}): {}",
                status.as_u16(),
                v.to_string().chars().take(160).collect::<String>()
            ));
        }
        if let Some(errs) = v.get("errors") {
            if !errs.is_null() && errs.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                return Err(format!("GraphQL 业务错误: {}", errs));
            }
        }
        Ok(v)
    }

    fn http_timeout(&self) -> u64 {
        30
    }

    /// 解析 `X-RateLimit-Reset`（Unix 时间戳）→ 距今秒数；解析不到返回 None。
    fn seconds_until_reset(&self, headers: &reqwest::header::HeaderMap) -> Option<u64> {
        let ts: i64 = headers
            .get("X-RateLimit-Reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let diff = ts - now;
        if diff <= 0 {
            Some(0)
        } else {
            Some(diff as u64)
        }
    }
}

/// `test_connection` 返回值：当前只承载 login，后续可扩展（scopes / 过期时间等）。
pub struct TestConnectionResult {
    pub login: String,
}

/// 合并多组搜索结果，按 `repo#number` 去重（后写入者覆盖前者）。
///
/// 与原 gh 实现保持完全一致：sync.rs 依赖这个签名。
pub fn merge_tasks_all(lists: Vec<Vec<RawTask>>) -> Vec<RawTask> {
    let mut map: HashMap<String, RawTask> = HashMap::new();
    for list in lists {
        for t in list {
            map.insert(format!("{}#{}", t.repo, t.number), t);
        }
    }
    map.into_values().collect()
}

/// 极简 URL 编码（仅编码 Search API 查询里 unsafe 字符），不依赖 `url` crate。
/// Search API 的 q 值已用 ASCII 字母/数字/冒号/空格，最小集够用。
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 隔离验证：不跑任何 issue 搜索，单独测 `fetch_prs` 能否在测试环境里正常拉到 PR。
    /// 用途：区分环境/rate-limit/网络三类根因。默认忽略，需时
    /// `cargo test --lib -- --ignored test_fetch_prs_isolated` 显式运行。
    #[test]
    #[ignore]
    fn test_fetch_prs_isolated() {
        let pat = std::env::var("TASKBOARD_TEST_PAT")
            .expect("需设置 TASKBOARD_TEST_PAT=<GitHub PAT>");
        let client = GitHubClient::new(pat).expect("客户端构造应成功");
        let repos = vec![
            "fad-backend".to_string(),
            "pq-backend".to_string(),
            "flutter-driver".to_string(),
            "foodsup-client".to_string(),
        ];
        let t0 = std::time::Instant::now();
        let prs = client
            .fetch_prs("FoodsUp-Inc", &repos)
            .expect("fetch_prs 不应报错");
        eprintln!(
            "[test] 隔离 fetch_prs 拉到 {} 个 PR，耗时 {:.1}s",
            prs.len(),
            t0.elapsed().as_secs_f64()
        );
        assert!(!prs.is_empty(), "隔离调用应至少拉到一个 PR");
        for pr in prs.iter().take(3) {
            assert!(pr.number > 0);
            assert!(pr.url.contains("github.com"));
            assert!(!pr.repo.is_empty(), "repo 应由调用方回填");
        }
    }

    #[test]
    fn test_urlencode() {
        assert_eq!(urlencode("org:Foo assignee:bar"), "org%3AFoo%20assignee%3Abar");
        assert_eq!(urlencode("a-b_c.d~e"), "a-b_c.d~e");
    }
}
