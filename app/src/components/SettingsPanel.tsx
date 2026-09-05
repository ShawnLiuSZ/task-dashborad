import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useI18n, type LangMode } from "../i18n";
import { formatCountdownSeconds } from "../utils/format";
import type { Account, DeviceLoginStart, Settings } from "../types";

interface Props {
  settings: Settings;
  onSaved: (s: Settings) => void;
  onClose: () => void;
  /** v0.3.16+：账号列表变更后通知 App.tsx 重新拉取 settings（显示新账号下拉）。 */
  onAccountsChanged?: () => void;
}

const PRESETS = [15, 30, 60, 120, 240];

export default function SettingsPanel({
  settings,
  onSaved,
  onClose,
  onAccountsChanged,
}: Props) {
  const { t, mode, setMode } = useI18n();
  const [minutes, setMinutes] = useState(settings.scheduleMinutes);
  const [ghPath, setGhPath] = useState(settings.ghPath);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // v0.3.22+：Project Status 诊断 + 项目列表。
  const [diagBusy, setDiagBusy] = useState(false);
  const [diagMsg, setDiagMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [diagAccountId, setDiagAccountId] = useState<number | null>(
    settings.accounts.find((a) => a.isDefault)?.id ?? settings.accounts[0]?.id ?? null,
  );
  const [projects, setProjects] = useState<any[]>([]);

  // v0.3.16+：多账号管理。
  const [accounts, setAccounts] = useState<Account[]>(settings.accounts);
  const [addingAccount, setAddingAccount] = useState(false);
  const [newLabel, setNewLabel] = useState("");
  const [newOrg, setNewOrg] = useState(settings.org || "");
  const [accountMsg, setAccountMsg] = useState<string | null>(null);
  const [testingAccountId, setTestingAccountId] = useState<number | null>(null);

  // v0.3.17+：GitHub OAuth Device Flow 登录（零注册，client_id 后端内置默认）。
  const [oauthPhase, setOauthPhase] = useState<"idle" | "code" | "success">("idle");
  const [oauthStart, setOauthStart] = useState<DeviceLoginStart | null>(null);
  const [oauthMsg, setOauthMsg] = useState<string | null>(null);
  const [oauthBusy, setOauthBusy] = useState(false);
  // 轮询取消令牌：面板关闭 / 重新发起时 +1，旧循环检测到变化即退出。
  const oauthRunRef = useRef(0);
  // v0.3.18：#6 授权倒计时（分:秒，无毫秒）。进入 code 阶段后每秒递减直到 0。
  const [oauthRemaining, setOauthRemaining] = useState(0);
  useEffect(() => {
    if (oauthPhase !== "code" || !oauthStart) {
      setOauthRemaining(0);
      return;
    }
    const runId = oauthRunRef.current;
    setOauthRemaining(oauthStart.expiresIn);
    const timer = setInterval(() => {
      if (oauthRunRef.current !== runId) {
        clearInterval(timer);
        return;
      }
      setOauthRemaining((prev) => Math.max(0, prev - 1));
    }, 1000);
    return () => clearInterval(timer);
  }, [oauthPhase, oauthStart]);

  const save = async () => {
    setSaving(true);
    setErr(null);
    try {
      onSaved(await api.saveSettings(minutes, ghPath));
    } catch (e) {
      setErr(String(e));
    } finally {
      setSaving(false);
    }
  };

  // v0.3.22+：诊断 Project Status 拉取（排查任务显示「未标注」）。
  const diagnoseProject = async () => {
    if (diagAccountId == null) {
      setDiagMsg({ ok: false, text: t("settings.projectDiagDefaultAcc") });
      return;
    }
    setDiagBusy(true);
    setDiagMsg(null);
    try {
      const res = await api.diagnoseProjectStatus(diagAccountId);
      const projs = (res.projects ?? []) as any[];
      const statusKeys = Object.keys(res.sample_statuses ?? {});
      setDiagMsg({
        ok: true,
        text:
          `组织 ${res.org} / 用户 ${res.login}\n` +
          `发现 ${projs.length} 个 Project：${projs.map((p: any) => p.name).join("、") || "（无）"}\n` +
          `已拉取 Status ${res.status_count} 条` +
          (statusKeys.length ? `，示例: ${statusKeys.slice(0, 8).join("、")}` : ""),
      });
    } catch (e) {
      setDiagMsg({ ok: false, text: String(e) });
    } finally {
      setDiagBusy(false);
    }
  };

  // 加载某账号的已存储项目列表。
  const loadProjects = async (accountId: number) => {
    try {
      setProjects(await api.listProjects(accountId));
    } catch {
      setProjects([]);
    }
  };

  // v0.3.16+：删除账号。
  const deleteAccount = async (id: number) => {
    if (!confirm(t("settings.deleteConfirm"))) {
      return;
    }
    setErr(null);
    try {
      await api.deleteAccount(id);
      setAccounts((prev) => prev.filter((a) => a.id !== id));
      onAccountsChanged?.();
    } catch (e) {
      setErr(String(e));
    }
  };

  // v0.3.16+：测试某账号的 PAT 是否仍有效。
  const testAccount = async (id: number) => {
    setTestingAccountId(id);
    setAccountMsg(null);
    setErr(null);
    try {
      const res = await api.testAccountPat(id);
      setAccountMsg(t("settings.testOk", { id, login: res.login }));
    } catch (e) {
      setAccountMsg(null);
      setErr(String(e));
    } finally {
      setTestingAccountId(null);
    }
  };

  // v0.3.16+：把某账号设为默认（同时切换激活）。
  const setDefault = async (id: number) => {
    setErr(null);
    try {
      await api.setDefaultAccount(id);
      onAccountsChanged?.();
      setAccountMsg(t("settings.defaultSet"));
    } catch (e) {
      setAccountMsg(null);
      setErr(String(e));
    }
  };

  // v0.3.17+：Device Flow 登录主流程（零注册：client_id 空时后端回落内置身份）。
  // 1) 申请设备码 → 2) 展示 user_code + 打开授权页
  // 3) 前端按 interval 轮询 → success/error。
  const runDeviceLogin = async (label: string, org: string) => {
    setOauthBusy(true);
    setOauthMsg(null);
    setErr(null);
    const runId = ++oauthRunRef.current;
    try {
      const st = await api.deviceLoginStart("");
      setOauthStart(st);
      setOauthPhase("code");
      // 打开浏览器到已预填 user_code 的授权页。
      void api.openInBrowser(st.verificationUriComplete);
      let interval = st.interval;
      while (oauthRunRef.current === runId) {
        await new Promise((r) => setTimeout(r, interval * 1000));
        if (oauthRunRef.current !== runId) return;
        // poll 的 client_id 传空 → 后端同样回落内置身份，与 start 一致。
        const res = await api.deviceLoginPoll("", st.deviceCode, org, label);
        if (res.status === "pending") continue;
        if (res.status === "slow_down") {
          interval += 5;
          continue;
        }
        if (res.status === "success") {
          setOauthPhase("success");
          setOauthMsg(t("settings.loginSuccess", { login: res.login }));
          onAccountsChanged?.();
          return;
        }
        setOauthPhase("idle");
        setOauthMsg(res.message || t("settings.loginFailed"));
        return;
      }
    } catch (e) {
      setOauthPhase("idle");
      setOauthMsg(String(e));
    } finally {
      if (oauthRunRef.current === runId) setOauthBusy(false);
    }
  };

  const cancelDeviceLogin = () => {
    oauthRunRef.current += 1;
    setOauthPhase("idle");
    setOauthStart(null);
    setOauthMsg(null);
    setOauthBusy(false);
  };

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3 className="modal-title">{t("settings.title")}</h3>

        {/* Issue #7：界面语言切换（跟随系统 / 简体中文 / English），即时生效。 */}
        <div className="field">
          <label>{t("settings.language")}</label>
          <select
            className="select"
            value={mode}
            onChange={(e) => setMode(e.target.value as LangMode)}
          >
            <option value="auto">{t("settings.langAuto")}</option>
            <option value="zh-CN">{t("settings.langZh")}</option>
            <option value="en-US">{t("settings.langEn")}</option>
          </select>
        </div>

        <div className="field">
          <label>{t("settings.syncInterval")}</label>
          <div className="row">
            <input
              className="input"
              type="number"
              min={5}
              value={minutes}
              onChange={(e) => setMinutes(Math.max(5, Number(e.target.value) || 5))}
            />
            <span className="muted">{t("unit.minutes")}</span>
          </div>
          <div className="presets">
            {PRESETS.map((p) => (
              <button
                key={p}
                className={`chip${minutes === p ? " on" : ""}`}
                onClick={() => setMinutes(p)}
              >
                {t("settings.presetMinutes", { n: p })}
              </button>
            ))}
          </div>
          <div className="muted small">{t("settings.intervalHint")}</div>
        </div>

        {/* v0.3.16+：多账号管理（v0.3.17 起登录走 Device Flow）。 */}
        <div className="field">
          <label>{t("settings.accountsTitle")}</label>
          {accounts.length === 0 ? (
            <div className="muted small">{t("settings.noAccounts")}</div>
          ) : (
            <div className="account-list">
              {accounts.map((a) => (
                <div key={a.id} className="account-row">
                  <div className="account-info">
                    <div className="account-row-head">
                      <strong>{a.label}</strong>
                      {a.isDefault && (
                        <span className="default-tag" title={t("settings.defaultTitle")}>
                          {t("settings.defaultTag")}
                        </span>
                      )}
                    </div>
                    <div className="muted small">
                      @{a.login}{a.org ? ` · ${a.org}` : ""} ·{" "}
                      {a.hasPat ? (
                        t("settings.authorized")
                      ) : (
                        <span className="warn">{t("settings.unauthorized")}</span>
                      )}
                    </div>
                  </div>
                  <div className="account-row-actions">
                    <button
                      className="btn small"
                      onClick={() => void testAccount(a.id)}
                      disabled={!a.hasPat || testingAccountId === a.id}
                      title={t("settings.testTitle")}
                    >
                      {testingAccountId === a.id ? t("settings.testing") : t("settings.test")}
                    </button>
                    {!a.isDefault && (
                      <button
                        className="btn small"
                        onClick={() => void setDefault(a.id)}
                        title={t("settings.setDefaultTitle")}
                      >
                        {t("settings.setDefault")}
                      </button>
                    )}
                    <button
                      className="btn small ghost"
                      onClick={() => void deleteAccount(a.id)}
                      disabled={a.isDefault}
                      title={
                        a.isDefault
                          ? t("settings.cantDeleteDefault")
                          : t("settings.deleteTitle")
                      }
                    >
                      {t("btn.delete")}
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}

          {!addingAccount ? (
            <button
              className="btn"
              style={{ marginTop: 8 }}
              onClick={() => setAddingAccount(true)}
            >
              {t("settings.addAccount")}
            </button>
          ) : (
            <div className="account-form">
              {oauthPhase === "idle" && (
                <>
                  <input
                    className="input wide"
                    placeholder={t("settings.labelPlaceholder")}
                    value={newLabel}
                    onChange={(e) => setNewLabel(e.target.value)}
                  />
                  <input
                    className="input wide"
                    placeholder={t("settings.orgPlaceholder")}
                    value={newOrg}
                    onChange={(e) => setNewOrg(e.target.value)}
                  />
                  <div className="row" style={{ marginTop: 6 }}>
                    <button
                      className="btn primary"
                      onClick={() => void runDeviceLogin(newLabel, newOrg)}
                      disabled={oauthBusy}
                    >
                      {oauthBusy ? t("settings.openingBrowser") : t("settings.authorizeLogin")}
                    </button>
                    <button
                      className="btn"
                      onClick={() => {
                        oauthRunRef.current += 1;
                        setAddingAccount(false);
                        setNewLabel("");
                        setNewOrg(settings.org || "");
                      }}
                    >
                      {t("btn.cancel")}
                    </button>
                  </div>
                  <div className="muted small" style={{ marginTop: 6 }}>
                    {t("settings.deviceFlowHint")}
                  </div>
                </>
              )}

              {oauthPhase === "code" && oauthStart && (
                <div className="device-flow">
                  <div className="muted small">{t("settings.enterCode")}</div>
                  <div className="user-code" title={t("settings.clickCopy")}>
                    {oauthStart.userCode}
                  </div>
                  <div className="row" style={{ marginTop: 8 }}>
                    <button
                      className="btn"
                      onClick={() => void api.openInBrowser(oauthStart.verificationUriComplete)}
                    >
                      {t("settings.reopenAuth")}
                    </button>
                    <button className="btn ghost" onClick={cancelDeviceLogin}>
                      {t("settings.cancelLogin")}
                    </button>
                  </div>
                  <div className="muted small" style={{ marginTop: 6 }}>
                    {t("settings.waitingAuth", { time: formatCountdownSeconds(oauthRemaining) })}
                  </div>
                </div>
              )}

              {oauthPhase === "success" && (
                <div className="device-flow">
                  <div className="banner ok inline">{oauthMsg}</div>
                  <button
                    className="btn"
                    style={{ marginTop: 8 }}
                    onClick={() => {
                      setOauthPhase("idle");
                      setOauthStart(null);
                      setOauthMsg(null);
                      setAddingAccount(false);
                      setNewLabel("");
                      setNewOrg(settings.org || "");
                    }}
                  >
                    {t("btn.done")}
                  </button>
                </div>
              )}

              {oauthMsg && oauthPhase === "idle" && (
                <div className="banner error inline">{oauthMsg}</div>
              )}
            </div>
          )}
          <div className="muted small" style={{ marginTop: 6 }}>
            {t("settings.accountsHint")}
          </div>
          {accountMsg && <div className="banner ok inline">{accountMsg}</div>}
        </div>

        <div className="field">
          <label>{t("settings.projectDiag")}</label>
          <div className="row" style={{ marginTop: 4 }}>
            <select
              className="select"
              value={diagAccountId ?? ""}
              onChange={(e) => {
                const id = Number(e.target.value);
                setDiagAccountId(id);
                void loadProjects(id);
              }}
            >
              {accounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.label}{a.isDefault ? " ★" : ""}
                </option>
              ))}
            </select>
            <button
              className="btn"
              onClick={() => { void diagnoseProject(); if (diagAccountId) void loadProjects(diagAccountId); }}
              disabled={diagBusy || accounts.length === 0}
            >
              {diagBusy ? t("settings.loading") : t("settings.projectDiag")}
            </button>
          </div>
          <div className="muted small">{t("settings.projectDiagHint")}</div>
          {projects.length > 0 && (
            <div className="project-list" style={{ marginTop: 6 }}>
              {projects.map((p: any) => (
                <div key={p.id} className="project-row">
                  <span className="project-name">{p.name}</span>
                  <span className="muted small">{p.numberOfItems} items · {p.ownerType}</span>
                </div>
              ))}
            </div>
          )}
          {diagMsg && (
            <div className={`banner ${diagMsg.ok ? "ok" : "error"} inline diag-banner`}>
              {diagMsg.text}
            </div>
          )}
        </div>

        <div className="field">
          <label>{t("settings.ghPathLabel")}</label>
          <input
            className="input wide"
            placeholder={t("settings.ghPathPlaceholder")}
            value={ghPath}
            onChange={(e) => setGhPath(e.target.value)}
          />
          <div className="muted small">{t("settings.ghPathHint")}</div>
        </div>

        <div className="field readonly">
          <label>{t("settings.orgLabel")}</label>
          <div className="muted">
            {settings.org}
            {t("settings.orgHint")}
          </div>
        </div>

        <div className="field readonly">
          <label>{t("settings.dbLabel")}</label>
          <div className="muted small path">{settings.dbPath}</div>
        </div>

        {err && <div className="banner error">{err}</div>}

        <div className="modal-actions">
          <button className="btn" onClick={onClose}>
            {t("btn.cancel")}
          </button>
          <button className="btn primary" onClick={save} disabled={saving}>
            {saving ? t("btn.saving") : t("btn.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
