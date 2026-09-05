# CLAUDE.md — TaskBoard 项目

本仓库是 **TaskBoard**：纯本地跨平台桌面（Windows / macOS / Linux，Tauri）任务看板，数据存本地 SQLite，**绝不写回 GitHub**。

## 自动维护看板（处理 issue 时请遵守）

本项目提供 TaskBoard MCP Server。v0.3.12 起 **MCP 已内置进 TaskBoard.app 二进制**（`taskboard mcp` 子命令，无 Python 依赖），已在 WorkBuddy 接入，命令见 `~/.workbuddy/mcp.json` 的 `taskboard` 项（`/Applications/TaskBoard.app/Contents/MacOS/taskboard` + `args:["mcp"]`）。未装 app 的便携场景才用兜底的 `mcp_server/server.py`。
当你处理本仓库相关的 GitHub issue 时，请按
[`mcp_server/AGENT_INSTRUCTIONS.md`](./mcp_server/AGENT_INSTRUCTIONS.md) 的触发规则，
通过 MCP 工具 **自动**把状态 / 会话 / 交接记录写回本地看板。

速记（详细见上面的指令文件）：

- 开始处理某 issue → `update_task_status(issue, "处理中")`
- 中途停止 / 切换任务 → `record_session(issue, <会话id>, "claude-code")`
- 用户说「生成交接任务」→ `record_handoff(issue, <已做/未做/卡点/如何恢复>)`
- 任务完成收尾 → `update_task_status(issue, "已完成")` + `clear_session(issue)`
- 查看现状 → `get_task_status(issue)`；列清单 → `list_my_tasks(ownership="notassignee")`

`issue` 支持 `repo#number` / `owner/repo#number` / GitHub URL 三种写法。
**只写本地看板，绝不调用 GitHub API、绝不改 Issue / Project / label / 评论。**

## 仓库结构

- `app/` — Tauri 应用（Rust 后端 `src-tauri/src/*` + React/TS 前端 `src/*`），数据存 SQLite
- `mcp_server/server.py` — 让 Agent 自动维护看板的 MCP Server（stdio，纯标准库；**便携/开发兜底**，常态请直接用 app 内 `taskboard mcp` 子命令）
- `app/src-tauri/src/mcp.rs` — 内置 MCP Server（Rust，随 app 二进制打包；`main.rs` 在 argv 含 `mcp` 时进入 stdio JSON-RPC 循环）
- `PRD.md` / `README.md` — 需求与变更记录
