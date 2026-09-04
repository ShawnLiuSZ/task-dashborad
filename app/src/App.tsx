import { useCallback, useEffect, useMemo, useState } from "react";
import { api, onSynced } from "./api";
import Board from "./components/Board";
import DetailPanel from "./components/DetailPanel";
import SettingsPanel from "./components/SettingsPanel";
import type { Account, Settings as SettingsT, Task } from "./types";

export function fmtTime(ts: number): string {
  if (!ts) return "从未";
  return new Date(ts * 1000).toLocaleString("zh-CN", { hour12: false });
}

export default function App() {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [settings, setSettings] = useState<SettingsT | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [ownership, setOwnership] = useState("");
  const [query, setQuery] = useState("");
  const [repo, setRepo] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [lastResult, setLastResult] = useState<string | null>(null);

  // 同步结果 banner 4 秒后自动消失（错误 banner 不受影响，由下次操作覆盖）。
  useEffect(() => {
    if (!lastResult) return;
    const t = setTimeout(() => setLastResult(null), 4000);
    return () => clearTimeout(t);
  }, [lastResult]);

  // v0.3.16+：根据当前 viewMode + activeAccountId 计算 listTasks 用的 accountId 参数。
  // - 'single' → activeAccountId（单账号视图）
  // - 'all'    → 0（聚合全部账号）
  const accountFilter = useMemo<number | null>(() => {
    if (!settings) return null;
    return settings.viewMode === "all" ? 0 : settings.activeAccountId;
  }, [settings]);

  const load = useCallback(async () => {
    try {
      setTasks(await api.listTasks(ownership || undefined, accountFilter));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [ownership, accountFilter]);

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

  // v0.3.16+：账号 id → Account 的映射，传给 Board 在卡片上显示账号徽章。
  const accountMap = useMemo(() => {
    const m = new Map<number, Account>();
    for (const a of settings?.accounts ?? []) {
      m.set(a.id, a);
    }
    return m;
  }, [settings]);

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

  // v0.3.16+：切换激活账号（单账号视图）。
  const handleSwitchAccount = async (id: number) => {
    setError(null);
    try {
      await api.setActiveAccount(id);
      await loadSettings();
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  // v0.3.16+：切换视图模式（single/all）。
  const handleSwitchView = async (mode: "single" | "all") => {
    setError(null);
    try {
      await api.setViewMode(mode);
      await loadSettings();
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const selectedTask = tasks.find((t) => t.key === selected) ?? null;

  // v0.3.16+：当前激活账号对象（用于顶栏显示）。
  const activeAccount = useMemo(
    () =>
      settings?.accounts.find((a) => a.id === settings.activeAccountId) ??
      null,
    [settings],
  );

  return (
    <div className="app">
      <header className="topbar">
        <div className="topbar-left">
          <span className="brand">TaskBoard</span>
          {/* v0.3.16+：账号下拉 + 视图模式切换。 */}
          <select
            className="select"
            value={settings?.viewMode ?? "single"}
            onChange={(e) =>
              void handleSwitchView(e.target.value as "single" | "all")
            }
            title="视图模式：单账号 / 全部账号"
          >
            <option value="single">单账号</option>
            <option value="all">全部账号</option>
          </select>
          {settings?.viewMode === "single" && (
            <select
              className="select"
              value={settings?.activeAccountId ?? 0}
              onChange={(e) => void handleSwitchAccount(Number(e.target.value))}
              title="切换激活账号"
            >
              {(settings?.accounts ?? []).length === 0 && (
                <option value={0}>（未配置）</option>
              )}
              {(settings?.accounts ?? []).map((a) => (
                <option key={a.id} value={a.id}>
                  {a.label} (@{a.login})
                </option>
              ))}
            </select>
          )}
          <span className="muted">
            {activeAccount
              ? `${activeAccount.login} @ ${activeAccount.org}`
              : settings?.hasPat === false
                ? "未配置 PAT"
                : "未登录"}
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

      <Board
        tasks={visible}
        selected={selected}
        onSelect={setSelected}
        accounts={accountMap}
      />

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
            void load();
          }}
          onClose={() => setShowSettings(false)}
          onAccountsChanged={() => {
            void loadSettings();
          }}
        />
      )}
    </div>
  );
}
