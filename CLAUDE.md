# CLAUDE.md — TaskBoard 项目（Claude Code 入口）

> Claude Code 在本仓库工作时加载本文件。**项目身份 / 硬约束 / 仓库结构 / KB 文档 / 分支 PR / 构建命令**等统一规则见 [`AGENTS.md`](./AGENTS.md)；本文件**仅承载 Claude Code 特有的看板自动维护约定**。

---

## 加载顺序（必读）

1. **共享规则** → [`AGENTS.md`](./AGENTS.md)
2. **MCP 工具契约与触发规则** → [`mcp_server/AGENT_INSTRUCTIONS.md`](./mcp_server/AGENT_INSTRUCTIONS.md)
3. **代码生成与文件风格**（与 GitHub Copilot 共用，可选）→ [`.github/copilot.instruction.md`](./.github/copilot.instruction.md)
4. **本文件特有部分**：下方「自动维护看板」

> 遇到冲突：**AGENTS.md > 本文件 > 用户口头指示**。

---

## 自动维护看板（Claude Code 特有约定）

本项目提供 TaskBoard MCP Server。v0.3.12 起 **MCP 已内置进 TaskBoard.app 二进制**（`taskboard mcp` 子命令，无 Python 依赖），已在 WorkBuddy 接入，命令见 `~/.workbuddy/mcp.json` 的 `taskboard` 项（`/Applications/TaskBoard.app/Contents/MacOS/taskboard` + `args:["mcp"]`）。未装 app 的便携场景才用兜底的 `mcp_server/server.py`。

当你处理本仓库相关的 GitHub issue 时，请按
[`mcp_server/AGENT_INSTRUCTIONS.md`](./mcp_server/AGENT_INSTRUCTIONS.md) 的触发规则，
通过 MCP 工具 **自动**把状态 / 会话 / 交接记录写回本地看板。

**MCP 工具速记表**见 [`AGENTS.md §7`](./AGENTS.md)（6 个工具：`update_task_status` / `get_task_status` / `record_session` / `record_handoff` / `clear_session` / `list_my_tasks`）。Claude Code 调用时 `<agent-name>` 固定传 `"claude-code"`，便于多 agent 写入时区分。

`issue` 支持 `repo#number` / `owner/repo#number` / GitHub URL 三种写法。
**只写本地看板，绝不调用 GitHub API、绝不改 Issue / Project / label / 评论。**

---

> 本文件由项目维护者维护。如发现本指令与 `AGENTS.md` / `AGENT_INSTRUCTIONS.md` 不一致，**以 `AGENTS.md` 为准**，并提交 PR 修复。