import { useState } from "react";
import { api } from "../api";
import { COLUMNS, OWNERSHIP_LABEL, type StatusKey, type Task } from "../types";
import { fmtTime } from "../App";

// 主流 coding agent 列表：供「中断会话」记录时标注来源。可按需增删。
// value 为规范化 slug（与 MCP/agent 自报名一致，便于存储与展示统一），
// label 为下拉里展示的名称。存储只认 value。
const AGENTS: { value: string; label: string }[] = [
  { value: "amazon-q", label: "Amazon Q" },
  { value: "augment", label: "Augment Code" },
  { value: "bolt", label: "Bolt.new" },
  { value: "chatgpt", label: "ChatGPT" },
  { value: "claude-code", label: "Claude Code" },
  { value: "cline", label: "Cline" },
  { value: "codebuddy", label: "CodeBuddy" },
  { value: "codeium", label: "Codeium" },
  { value: "codex", label: "Codex (OpenAI)" },
  { value: "codestral", label: "Codestral" },
  { value: "cody", label: "Sourcegraph Cody" },
  { value: "continue", label: "Continue" },
  { value: "copilot", label: "GitHub Copilot" },
  { value: "cursor", label: "Cursor" },
  { value: "deepseek", label: "DeepSeek" },
  { value: "devin", label: "Devin" },
  { value: "doubao", label: "豆包 (Doubao)" },
  { value: "factory", label: "Factory Droid" },
  { value: "gemini-cli", label: "Gemini CLI" },
  { value: "glm", label: "智谱 GLM" },
  { value: "goose", label: "Goose" },
  { value: "grok", label: "Grok (xAI)" },
  { value: "helix", label: "Helix CLI" },
  { value: "kimi", label: "Kimi" },
  { value: "llama", label: "Llama (Meta)" },
  { value: "opencode", label: "OpenCode" },
  { value: "openhands", label: "OpenHands" },
  { value: "phind", label: "Phind" },
  { value: "qwen-code", label: "Qwen Code" },
  { value: "replit", label: "Replit Agent" },
  { value: "roo-code", label: "Roo Code" },
  { value: "tabnine", label: "Tabnine" },
  { value: "tongyi", label: "通义灵码" },
  { value: "trae", label: "Trae" },
  { value: "v0", label: "Vercel v0" },
  { value: "windsurf", label: "Windsurf" },
  { value: "workbuddy", label: "WorkBuddy" },
  { value: "zcode", label: "ZCode" },
  { value: "aider", label: "Aider" },
];

interface Props {
  task: Task;
  onClose: () => void;
  onChanged: () => void;
}

export default function DetailPanel({ task, onClose, onChanged }: Props) {
  const [busy, setBusy] = useState(false);
  const [sessionInput, setSessionInput] = useState(task.sessionId ?? "");
  const [agent, setAgent] = useState(task.sessionAgent ?? "claude-code");
  const [err, setErr] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [handoff, setHandoff] = useState(task.handoff ?? "");

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setErr(null);
    try {
      await fn();
      onChanged();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const copySession = async () => {
    if (!task.sessionId) return;
    await navigator.clipboard.writeText(task.sessionId);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <aside className="detail">
      <div className="detail-head">
        <div>
          <span className="repo">{task.repo}</span>
          <span className="num">#{task.number}</span>
        </div>
        <button className="btn ghost" onClick={onClose}>
          关闭
        </button>
      </div>

      <h2 className="detail-title">{task.title}</h2>

      <div className="tags">
        <span className={`own own-${task.ownership}`}>
          {OWNERSHIP_LABEL[task.ownership]}
        </span>
        {task.candidateDone && <span className="candidate-tag">GitHub 已关闭 · 待确认</span>}
        <span className="muted small">更新于 {task.updatedAt ?? "—"}</span>
      </div>

      <section className="detail-block">
        <div className="block-title">执行状态</div>
        <div className="seg">
          {COLUMNS.map((c) => (
            <button
              key={c.key}
              className={`seg-btn${task.status === c.key ? " on" : ""}`}
              disabled={busy}
              onClick={() => run(() => api.updateStatus(task.key, c.key as StatusKey))}
            >
              {c.label}
            </button>
          ))}
        </div>
      </section>

      <section className="detail-block">
        <div className="block-title">中断会话</div>
        <div className="row">
          <input
            className="input"
            placeholder="session id"
            value={sessionInput}
            onChange={(e) => setSessionInput(e.target.value)}
          />
          <select className="select" value={agent} onChange={(e) => setAgent(e.target.value)}>
            {AGENTS.map((a) => (
              <option key={a.value} value={a.value}>
                {a.label}
              </option>
            ))}
          </select>
        </div>
        <div className="row">
          <button
            className="btn"
            disabled={busy || !sessionInput.trim()}
            onClick={() =>
              run(() => api.recordSession(task.key, sessionInput.trim(), agent))
            }
          >
            记录
          </button>
          <button
            className="btn"
            disabled={busy || !task.sessionId}
            onClick={() => run(() => api.clearSession(task.key))}
          >
            清空
          </button>
          <button className="btn" disabled={!task.sessionId} onClick={copySession}>
            {copied ? "已复制" : "复制"}
          </button>
        </div>
        {task.sessionId && (
          <div className="muted small">
            {task.sessionAgent || "未标注"} · 记录于 {fmtTime(task.sessionAt ?? 0)}
          </div>
        )}
      </section>

      <section className="detail-block">
        <div className="block-title">交接任务</div>
        <textarea
          className="input wide"
          rows={3}
          placeholder="接入 claude / codex 等 agent 后，由其识别「生成交接任务」类意图并写入；也可在此手动记录"
          value={handoff}
          onChange={(e) => setHandoff(e.target.value)}
        />
        <div className="row">
          <button
            className="btn"
            disabled={busy || !handoff.trim()}
            onClick={() => run(() => api.recordHandoff(task.key, handoff.trim()))}
          >
            保存
          </button>
          {task.handoff && handoff.trim() !== task.handoff && (
            <button className="btn ghost" onClick={() => setHandoff(task.handoff ?? "")}>
              还原
            </button>
          )}
        </div>
        {task.handoff && (
          <div className="muted small top-gap">已记录交接详情（共 {task.handoff.length} 字）</div>
        )}
      </section>

      <section className="detail-block">
        <div className="block-title">GitHub</div>
        <div className="row">
          <button className="btn" onClick={() => void api.openInBrowser(task.url)}>
            在浏览器打开
          </button>
          {task.prNumber > 0 && task.prUrl && (
            <button className="btn" onClick={() => void api.openInBrowser(task.prUrl)}>
              PR #{task.prNumber}
            </button>
          )}
          {task.latestCommentUrl && (
            <button className="btn" onClick={() => void api.openInBrowser(task.latestCommentUrl)}>
              最新评论
            </button>
          )}
        </div>
        <div className="muted small top-gap">
          {task.assignees
            ? `分配人：${task.assignees.split(",").filter(Boolean).map((a) => `@${a}`).join(" ")}`
            : "无人认领"}
          {task.mentioned && " · 评论区有人 @我"}
        </div>
        {task.branch && (
          <div className="branch-line top-gap">🌿 分支：{task.branch}</div>
        )}
        <div className="muted small top-gap">
          会话与状态只存在本地，不会写回 GitHub
        </div>
      </section>

      {err && <div className="banner error">{err}</div>}
    </aside>
  );
}
