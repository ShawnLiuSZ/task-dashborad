import { useCallback, useEffect, useMemo, useState } from "react";
import { api, onSynced } from "./api";
import Board from "./components/Board";
import DetailPanel from "./components/DetailPanel";
import SettingsPanel from "./components/SettingsPanel";
import type { Settings, Task } from "./types";

export function fmtTime(ts: number): string {
  if (!ts) return "从未";
  return new Date(ts * 1000).toLocaleString("zh-CN", { hour12: false });
}

export default function App() {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [ownership, setOwnership] = useState("");
  const [query, setQuery] = useState("");
  const [repo, setRepo] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [lastResult, setLastResult] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setTasks(await api.listTasks(ownership || undefined));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [ownership]);

  const loadSettings = useCallback(async () => {
    try {
      setSettings(await api.getSettings());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    void loadSettings();
    const un = onSynced((r) => {
      void load();
      void loadSettings();
      const warn = r.warning ? ` · ⚠️ ${r.warning}` : "";
      const prune = r.pruned > 0 ? ` · 清理已完成 ${r.pruned}` : "";
      setLastResult(`新增 ${r.added} · 更新 ${r.updated} · 候选完成 ${r.candidateDone}${prune}${warn}`);
    });
    return () => {
      void un.then((f) => f());
    };
  }, [load, loadSettings]);

  // 仓库列表（去重排序），用于仓库筛选下拉。
  const repos = useMemo(
    () => [...new Set(tasks.map((t) => t.repo))].sort(),
    [tasks],
  );

  // 前端实时过滤：归属由后端 list_tasks 已筛；此处叠加 仓库 + 关键词（仓库/编号/标题）。
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return tasks.filter((t) => {
      if (repo && t.repo !== repo) return false;
      if (!q) return true;
      const hay = `${t.repo}#${t.number} ${t.title}`.toLowerCase();
      return hay.includes(q);
    });
  }, [tasks, repo, query]);

  const doSync = async () => {
    setSyncing(true);
    setError(null);
    try {
      const r = await api.syncNow();
      const warn = r.warning ? ` · ⚠️ ${r.warning}` : "";
      const prune = r.pruned > 0 ? ` · 清理已完成 ${r.pruned}` : "";
      setLastResult(`新增 ${r.added} · 更新 ${r.updated} · 候选完成 ${r.candidateDone}${prune}${warn}`);
      await load();
      await loadSettings();
    } catch (e) {
      setError(String(e));
    } finally {
      setSyncing(false);
    }
  };

  const selectedTask = tasks.find((t) => t.key === selected) ?? null;

  return (
    <div className="app">
      <header className="topbar">
        <div className="topbar-left">
          <span className="brand">TaskBoard</span>
          <span className="muted">
            {settings?.login ? `${settings.login} @ ${settings.org}` : settings?.hasPat === false ? "未配置 PAT" : "未登录"}
            {" · "}
            共 {visible.length} 条
          </span>
        </div>

        <div className="topbar-right">
          <span className="muted small">上次同步 {fmtTime(settings?.lastSyncAt ?? 0)}</span>
          <button className="btn" onClick={() => setShowSettings(true)}>
            设置
          </button>
          <button className="btn primary" onClick={doSync} disabled={syncing}>
            {syncing ? "同步中…" : "立即同步"}
          </button>
        </div>
      </header>

      <div className="toolbar">
        <input
          className="input"
          placeholder="搜索仓库 / 编号 / 标题…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <select
          className="select"
          value={repo}
          onChange={(e) => setRepo(e.target.value)}
          title="按仓库筛选"
        >
          <option value="">全部仓库</option>
          {repos.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        <select
          className="select"
          value={ownership}
          onChange={(e) => setOwnership(e.target.value)}
          title="按归属筛选"
        >
          <option value="">全部归属</option>
          <option value="assigned">分配给我</option>
          <option value="notassignee">无人认领</option>
          <option value="assigned-others">分配给他人</option>
        </select>
        {(query || repo || ownership) && (
          <button
            className="btn ghost"
            onClick={() => {
              setQuery("");
              setRepo("");
              setOwnership("");
            }}
            title="清除筛选"
          >
            重置
          </button>
        )}
      </div>

      {(error || lastResult) && (
        <div className="banner-row">
          {error && <div className="banner error">{error}</div>}
          {!error && lastResult && <div className="banner ok">{lastResult}</div>}
        </div>
      )}

      <Board tasks={visible} selected={selected} onSelect={setSelected} />

      {selectedTask && (
        <>
          <div
            className="detail-backdrop"
            onClick={() => setSelected(null)}
            title="点击空白处关闭"
          />
          <DetailPanel
            task={selectedTask}
            onClose={() => setSelected(null)}
            onChanged={() => {
              void load();
            }}
          />
        </>
      )}

      {showSettings && settings && (
        <SettingsPanel
          settings={settings}
          onSaved={(s) => {
            setSettings(s);
            setShowSettings(false);
          }}
          onClose={() => setShowSettings(false)}
        />
      )}
    </div>
  );
}
