import { useCallback, useEffect, useMemo, useState } from "react";
import { api, onSynced } from "./api";
import { fmtTime, I18nProvider, useI18n } from "./i18n";
import Board from "./components/Board";
import DetailPanel from "./components/DetailPanel";
import SettingsPanel from "./components/SettingsPanel";
import AboutPanel from "./components/AboutPanel";
import AccountsPanel from "./components/AccountsPanel";
import SyncLogsPanel from "./components/SyncLogsPanel";
import NotesPanel from "./components/NotesPanel";
import type { Account, BoardMode, ProjectStatus, Settings as SettingsT, Task } from "./types";

export default function App() {
  return (
    <I18nProvider>
      <BoardApp />
    </I18nProvider>
  );
}

function BoardApp() {
  const { lang, t } = useI18n();
  const [tasks, setTasks] = useState<Task[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [settings, setSettings] = useState<SettingsT | null>(null);
  const [activeModal, setActiveModal] = useState<"settings" | "about" | null>(null);
  const [showAccounts, setShowAccounts] = useState(false);
  const [showSyncLogs, setShowSyncLogs] = useState(false);
  const [ownership, setOwnership] = useState("");
  const [query, setQuery] = useState("");
  const [repo, setRepo] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [lastResult, setLastResult] = useState<string | null>(null);
  const [projectStatuses, setProjectStatuses] = useState<ProjectStatus[]>([]);

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

  const loadProjectStatuses = useCallback(async () => {
    try {
      const activeId = settings?.activeAccountId;
      if (activeId) {
        const all = await api.listProjectStatuses(activeId);
        // 按 project_github_id 分组，取条目数最多的项目（主项目）的状态
        const byProject = new Map<string, typeof all>();
        for (const ps of all) {
          const arr = byProject.get(ps.projectGithubId) ?? [];
          arr.push(ps);
          byProject.set(ps.projectGithubId, arr);
        }
        // 取条目最多的项目
        let best: typeof all = [];
        for (const arr of byProject.values()) {
          if (arr.length > best.length) best = arr;
        }
        setProjectStatuses(best);
      }
    } catch (e) {
      console.warn("加载项目状态选项失败:", e);
    }
  }, [settings?.activeAccountId]);

  useEffect(() => {
    void load();
    void loadSettings();
    const un = onSynced((r) => {
      void load();
      void loadSettings();
      void loadProjectStatuses();
      const warn = r.warning ? ` · ⚠️ ${r.warning}` : "";
      const prune = r.pruned > 0 ? ` · ${t("sync.pruned", { n: r.pruned })}` : "";
      setLastResult(
        `${t("sync.result", { added: r.added, updated: r.updated, done: r.candidateDone })}${prune}${warn}`,
      );
    });
    return () => {
      void un.then((f) => f());
    };
  }, [load, loadSettings, t]);

  // settings 加载完成后拉取项目 Status 选项
  useEffect(() => {
    void loadProjectStatuses();
  }, [loadProjectStatuses]);

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
      const prune = r.pruned > 0 ? ` · ${t("sync.pruned", { n: r.pruned })}` : "";
      setLastResult(
        `${t("sync.result", { added: r.added, updated: r.updated, done: r.candidateDone })}${prune}${warn}`,
      );
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

  return (
    <div className="app">
      <header className="topbar">
        <div className="topbar-left">
          {/* v0.3.16+：账号下拉 + 视图模式切换。 */}
          <select
            className="select"
            value={settings?.viewMode ?? "single"}
            onChange={(e) =>
              void handleSwitchView(e.target.value as "single" | "all")
            }
            title={t("topbar.viewModeTitle")}
          >
            <option value="single">{t("topbar.singleAccount")}</option>
            <option value="all">{t("topbar.allAccounts")}</option>
          </select>
          {settings?.viewMode === "single" && (
            <select
              className="select"
              value={settings?.activeAccountId ?? 0}
              onChange={(e) => void handleSwitchAccount(Number(e.target.value))}
              title={t("topbar.switchAccount")}
            >
              {(settings?.accounts ?? []).length === 0 && (
                <option value={0}>{t("topbar.noAccounts")}</option>
              )}
              {(settings?.accounts ?? []).map((a) => (
                <option key={a.id} value={a.id}>
                  @{a.login}
                  {a.org ? ` (${a.org})` : ""}
                </option>
              ))}
            </select>
          )}
          <span className="muted">
            {t("topbar.totalCount", { n: visible.length })}
          </span>
        </div>

        <div className="topbar-right">
          <span className="muted small">
            {t("topbar.lastSync", { time: fmtTime(settings?.lastSyncAt ?? 0, lang) })}
          </span>
          <button className="btn" onClick={() => setActiveModal(activeModal === "about" ? null : "about")}>
            {t("btn.about")}
          </button>
          <button className="btn" onClick={() => setActiveModal(activeModal === "settings" ? null : "settings")}>
            {t("btn.settings")}
          </button>
          <button className="btn" onClick={() => setShowAccounts(true)}>
            {t("btn.accounts")}
          </button>
          <button className="btn" onClick={() => setShowSyncLogs(true)}>
            同步日志
          </button>
          <button className="btn primary" onClick={doSync} disabled={syncing}>
            {syncing ? t("btn.syncing") : t("btn.syncNow")}
          </button>
        </div>
      </header>

      <div className="toolbar">
        <input
          className="input"
          placeholder={t("search.placeholder")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <select
          className="select"
          value={repo}
          onChange={(e) => setRepo(e.target.value)}
          title={t("filter.byRepo")}
        >
          <option value="">{t("filter.allRepos")}</option>
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
          title={t("filter.byOwnership")}
        >
          <option value="">{t("filter.allOwnership")}</option>
          <option value="assigned">{t("ownership.assigned")}</option>
          <option value="notassignee">{t("ownership.notassignee")}</option>
          <option value="assigned-others">{t("ownership.assigned-others")}</option>
        </select>
        {(query || repo || ownership) && (
          <button
            className="btn ghost"
            onClick={() => {
              setQuery("");
              setRepo("");
              setOwnership("");
            }}
            title={t("filter.clear")}
          >
            {t("btn.reset")}
          </button>
        )}

        {/* v0.3.21+：看板列模式切换（Project Status 列视图） */}
        <select
          className="select"
          value={settings?.boardMode ?? "project"}
          onChange={(e) => {
            const mode = e.target.value as BoardMode;
            if (mode !== settings?.boardMode) {
              void api.setBoardMode(mode);
              void loadSettings();
            }
          }}
          title={t("settings.boardModeTitle")}
        >
          <option value="project">{t("settings.boardModeProject")}</option>
        </select>
      </div>

      {(error || lastResult) && (
        <div className="banner-row">
          {error && <div className="banner error">{error}</div>}
          {!error && lastResult && <div className="banner ok">{lastResult}</div>}
        </div>
      )}

      <div className="main-layout">
        <NotesPanel />
        <div className="board-wrap">
          <Board
            tasks={visible}
            selected={selected}
            onSelect={setSelected}
            accounts={accountMap}
            boardMode={settings?.boardMode ?? "project"}
            projectStatuses={projectStatuses}
          />
        </div>
      </div>

      {selectedTask && (
        <>
          <div
            className="detail-backdrop"
            onClick={() => setSelected(null)}
            title={t("detail.clickBackdropClose")}
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

      {activeModal === "settings" && settings && (
        <SettingsPanel
          settings={settings}
          onSaved={(s) => {
            setSettings(s);
            setActiveModal(null);
            void load();
          }}
          onClose={() => setActiveModal(null)}
        />
      )}

      {showAccounts && settings && (
        <AccountsPanel
          settings={settings}
          onClose={() => setShowAccounts(false)}
          onAccountsChanged={() => {
            void loadSettings();
          }}
        />
      )}

      {activeModal === "about" && (
        <AboutPanel
          onClose={() => setActiveModal(null)}
        />
      )}

      {showSyncLogs && (
        <SyncLogsPanel
          onClose={() => setShowSyncLogs(false)}
        />
      )}
    </div>
  );
}
