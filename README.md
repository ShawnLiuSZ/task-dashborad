# GitHub 任务看板 · TaskBoard

每天自动汇聚 GitHub 上「分配给我 / 与我相关」的任务到**本地 Mac 应用**看板，状态随 AI 执行自动流转，并支持记录可恢复的中断会话（session id）。

目前处于 ***开发者预览*** 阶段，正在快速迭代。未来将出现破坏兼容性的变更。

> **最终形态（v0.3）**：已落地为**本地 Tauri 桌面应用**（`app/` 目录），数据存本地 SQLite，**不创建 GitHub Issue / Project，不写回 GitHub**。此前 PRD 讨论的「GitHub Projects v2 看板」方案因组织限制与个人偏好已放弃，演进记录见 [`PRD.md`](./PRD.md)。

## 应用：TaskBoard（Mac）

原生 macOS 菜单栏常驻应用，前端 React、后端 Rust（rusqlite 本地数据库），打包为 `.app`。

### 构建与运行

```bash
cd app
npm install            # 首次安装前端依赖
npm run tauri dev      # 开发模式（前端热更新）
npm run tauri build    # 产出 release .app + .dmg
```

产物位置：`app/src-tauri/target/release/bundle/macos/TaskBoard.app`

> 未配置 Apple 开发者签名。首次打开若被 Gatekeeper 拦截：右键「打开」，或在终端执行
> `xattr -cr "/path/to/TaskBoard.app"` 后双击。

### 在线自动打包（GitHub Actions，发布 Release 触发）

`.github/workflows/release.yml` 在**发布 GitHub Release（点 Publish）时**，用 GitHub 托管 runner 自动构建三端安装包并上传为 Release 附件，无需本地机器：

| 平台 | runner | 产物 |
|---|---|---|
| macOS（Apple Silicon） | `macos-latest` | `.dmg`（含 `.app`） |
| Windows（x64） | `windows-latest` | `.exe`（NSIS 安装器） |
| Linux（x64） | `ubuntu-22.04` | `.deb` + `.AppImage` |

**发布流程**：

```bash
# 1. 把三处 version 对齐（当前 0.1.0）：tauri.conf.json / Cargo.toml / package.json
# 2. 打 tag 并推送
git tag v0.1.0 && git push origin v0.1.0
# 3. 在 GitHub 上基于该 tag 创建 Release → Publish → 自动触发打包
#    （也可在 Actions 页手动 Run workflow 兜底）
```

**前提说明**：

- 已在 `tauri.conf.json` 的 `bundle.icon` 加入 `icons/icon.ico`（Windows 安装器图标必需，从 `icon-512.png` 生成）。
- 三端产物**均未做代码签名**：macOS 首次打开需右键「打开」/ `xattr -cr`；Windows 会弹 SmartScreen 提示（点「更多信息 → 仍要运行」）；Linux 无签名要求。
- 本仓库是**私有/个人仓库**，`GITHUB_TOKEN` 已授予 `contents: write` 用于上传附件；公开仓库需注意 Release 附件对外可见。
- 如日后需要 Intel Mac，在 workflow 的 matrix 里给 `macos-latest` 增加 `--target x86_64-apple-darwin` 一项即可。

### 使用

| 能力 | 操作 |
|---|---|
| 菜单栏 | 单击切换看板窗口；右键菜单含「显示看板 / 立即同步 / 退出」 |
| **定时更新** | 设置里调「定时同步间隔」（5–240 分钟），应用常驻时自动按间隔拉取 |
| **手动更新** | 主界面右上「立即同步」按钮 |
| 四态看板 | 待处理 / 处理中 / 已处理 / 已完成；点卡片在右栏切换状态 |
| 远程状态联动 | GitHub 已关闭的 issue **自动归入「已完成」**（以远程真实状态为准，覆盖本地手动态）；仍打开但不再与你相关的任务自动移出看板 |
| 归属筛选 | 顶部下拉按 `分配给我` / `无人认领` / `分配给他人` 过滤 |
| 搜索 / 仓库筛选 | 顶部搜索框按 **仓库名 / 编号 / 标题** 实时过滤；仓库下拉按仓库隔离；右侧「重置」一键清除所有筛选 |
| 中断会话 | 选中卡片 → 输入 session id + 选 agent（claude-code / workbuddy / doubao / opencode / codex / zcode / gemini-cli / cursor / aider / qwen-code 等）→ 记录；可复制、可清空 |
| 交接任务 | 选中卡片 → 「交接任务」区块录入详情并保存（后续可由接入的 agent 在识别「生成交接任务」类意图时自动写入） |
| 本地数据 | `~/Library/Application Support/com.liushizhao.taskboard/taskboard.db` |

### 关键约束

> **session id 与任务状态只存本地 SQLite，绝不写回 GitHub。** 不创建 Issue、不创建 Project、不改 Issue 标题 / label / 评论。

## 设计要点（纯本地，与 GitHub 解耦）

- **拉取**：`gh api` 调 Search API，**合并多个稳定查询源按 `repo#number` 去重**（已排除 PR）：`assignee:<login>`（分配给我，权威）+ `author:<login>`（我创建）+ `mentions:<login>`（@我）+ `commenter:<login>`（我评论）+ `involves:<login>`（兜底）。
  - ⚠️ **为何多源而非单一 `involves:`**：GitHub `involves:` 搜索结果**非确定性抖动**——总数恒为 76，但成员会随机漏拉（如 `fad-backend#1066/#1071/#1072/#1100/#1138/#1139`、`pq-backend#259` 曾在某次 `involves` 结果中缺失）。多个稳定源取并集，任一源漏拉都会被其他源补回，看板不再随机缺任务。
- **归属三分**：无 assignee → `notassignee`；含我 → `assigned`；他人 → `assigned-others`（GitHub Search 不支持 `-no:assignee`，须读 `assignees` 数组判定，不能用查询语法区分）
- **状态四态**：`todo` / `doing` / `processed` / `done`，本地维护，同步时不被远程覆盖
- **closed 处理**：GitHub 已关闭的任务 → 状态置「已完成」(`done`)，并标 `candidate_done` 作「远程已关闭、待本地确认」提示；以远程真实状态为权威，覆盖本地手动态（即便曾被标为处理中/已处理，关闭即视为做完）
- **PR 关联**：卡片记录对应 PR 的编号与链接。实现上**不使用 Search API 的 `is:pr`**（限流极严、突发易挂起拖垮同步），改为按「用户 issue 实际所在仓库」调用 REST `repos/{org}/{repo}/pulls?state=all` 拉取各仓库 PR，解析正文 `#N` / `owner/repo#N` 反向关联到看板卡片（`best-effort`：单页/单仓失败仅跳过，不影响其余）
- **定时**：应用内 ticker（应用常驻菜单栏时持续生效；退出应用即停止）

## 实测基线（2026-09-03 / 04）

| 项 | 结果 |
|---|---|
| 与我相关（org 内 open issue） | 76 条 = **58 分配给我** + **13 无人认领** + 5 分配给他人 |
| 任务仓库 | `fad-backend` 53 · `pq-backend` 6 |
| `gh` 凭据 | v2.97.0 已登录，scopes 含 `repo` `read:org` |

## MCP Server（v0.3.10 新增 · v0.3.12 集成进 app 二进制）

PRD §6 规划了「MCP Server + Skill」让 AI Agent 在执行任务时自动维护看板。本版落地 **MCP Server** 部分（D5：先 MCP，后包 Skill）。

> **与 PRD 的关键偏离**：PRD 原设想把 session / 状态写到 **GitHub Project v2 自定义字段**；但本 App 的最终形态是**纯本地 SQLite、绝不写回 GitHub**。因此 MCP Server 直接读写本地 `taskboard.db`，零 GitHub 调用——这是 PRD 设计在当前架构下的正确适配。

**两种运行形态（同一份工具契约）**：

1. **内置二进制（推荐，v0.3.12 起）**：`taskboard` 二进制新增 `mcp` 子命令——`main.rs` 在 argv 含 `mcp` 时直接进入 stdio JSON-RPC 循环，**不启动 GUI**。它复用与 App **完全相同的** `db.rs` schema 与同一份 `taskboard.db`，**零 Python 依赖、无散落文件夹、无 schema 漂移**。装了 app 即自带 MCP，mcp.json 直接指向 app 内二进制即可（见下方配置）。
2. **独立 `server.py`（便携 / 开发兜底）**：`mcp_server/server.py` 仍保留——**仅用 Python 标准库**（手写 JSON-RPC 2.0 + LSP 风格 `Content-Length` 分帧），无第三方依赖。适用于非 macOS / 未装 app 时让 Agent 读写同一数据库；其工具与内置二进制保持兼容。数据库路径默认 `~/Library/Application Support/com.liushizhao.taskboard/taskboard.db`（内置二进制同理），可用环境变量 `TASKBOARD_DB` 覆盖；启动时会幂等补齐 `branch` / `handoff` 两列（与 App 的 `db.rs::init` 迁移一致），故**即使 App 还没启动过也能直接用**。

**提供的工具**（与 PRD §6.2 对齐）：

| 工具 | 入参 | 说明 |
|---|---|---|
| `list_my_tasks` | `status?` / `ownership?` | 列出看板任务，可按四态 / 归属过滤 |
| `get_task_status` | `issue` | 查询某任务当前状态 + 已记录的 session / handoff |
| `update_task_status` | `issue`, `status` | 改本地看板状态（todo/doing/processed/done 或中文四态） |
| `record_session` | `issue`, `session_id`, `agent?` | 记录中断会话 id（不碰 GitHub） |
| `record_handoff` | `issue`, `text` | 记录「交接任务」详情（不碰 GitHub） |
| `clear_session` | `issue` | 任务完成后清空 session 字段（保留 session_at 审计） |

`issue` 接受 `repo#number` / `owner/repo#number` / GitHub URL 三种形式。`status` 接受 `todo`/`doing`/`processed`/`done` 或中文「待处理/处理中/已处理/已完成」。

**Agent 使用范式**（对应 PRD §6.4 时序）：任务开始 → `update_task_status(issue,"处理中")`；中途停止 → `record_session(issue, <会话id>, <agent>)`；识别到「生成交接任务」→ `record_handoff(issue, <详情>)`；完成 → `update_task_status(issue,"已完成")` → `clear_session(issue)`。

**接入各 Agent（配置 snippet）**：内置二进制已注册进 WorkBuddy 的 `~/.workbuddy/mcp.json`（`taskboard` 项）。其他本地 Agent 在其 MCP 配置里加同一条即可，例如 claude-code 的 `~/.claude.json`：

```json
{
  "mcpServers": {
    "taskboard": {
      "type": "stdio",
      "command": "/Applications/TaskBoard.app/Contents/MacOS/taskboard",
      "args": ["mcp"]
    }
  }
}
```

> 路径说明：上述为默认安装位置（`/Applications/TaskBoard.app/...`）。若你安装到了其他位置，把 `command` 改成实际 `TaskBoard.app/Contents/MacOS/taskboard` 的绝对路径即可。**未安装 app、改用 `server.py` 兜底**时，配置改为 `"command": "python3", "args": ["/path/to/mcp_server/server.py"]`。
>
> 注：已注册的 WorkBuddy MCP 需在其连接器页「信任」后才会激活；codex / cursor 等按各自 MCP 配置位置填入上述 `command` + `args` 即可。

### 让 Agent 真正自动接上（触发逻辑）

MCP Server 只提供工具；要让 Agent 在「开始 / 中断 / 说『生成交接任务』/ 完成」时**自动**调用，需要一份**触发规则**被 Agent 加载。已在仓库内置：

- **`mcp_server/AGENT_INSTRUCTIONS.md`** —— 跨 Agent 通用的指令规范：触发时机 → 精确 MCP 工具调用、issue 引用格式、状态枚举、会话 id 来源约定。可直接整体喂给 claude-code / codex / opencode / zcode / helix / cursor / doubao。
- **`CLAUDE.md`**（仓库根） —— 给 claude-code 的自动加载入口，指向上述指令文件并给出速记规则；在本仓库跑 claude-code 时会自动生效。
- 其他 Agent：把 `AGENT_INSTRUCTIONS.md` 的内容并入其 system prompt / 项目指令即可（codex 的 `AGENTS.md`、helix 的技能/系统提示、cursor 的 `.cursorrules` 等同理）。

> 这样即完成 PRD D5 的「先 MCP，后包 Skill」：MCP 是能力层（已就位），指令文件是「Skill」等价物（跨 Agent 复用），Agent 侧按意图编排调用。

## 文档

- [`PRD.md`](./PRD.md) — 需求文档与决策演进（含已放弃的 Projects v2 方案、归属维度设计、API 避坑点）
- [`docs/CHANGELOG.md`](./docs/CHANGELOG.md) — 各版本的更新说明与修复记录（v0.3.1 → v0.3.15）
- [`docs/v0.3.15-pat-auth.md`](./docs/v0.3.15-pat-auth.md) — v0.3.15 PAT 认证与 visual polish 设计文档（gh 替换、卡片配色、多账号规划）

> 版本 v0.3.14 · 本地 Mac App，2026-09-04

