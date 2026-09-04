import { useRef, useState } from "react";
import { api } from "../api";
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
  const [minutes, setMinutes] = useState(settings.scheduleMinutes);
  const [ghPath, setGhPath] = useState(settings.ghPath);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // v0.3.16+：多账号管理。
  const [accounts, setAccounts] = useState<Account[]>(settings.accounts);
  const [addingAccount, setAddingAccount] = useState(false);
  const [newLabel, setNewLabel] = useState("");
  const [newOrg, setNewOrg] = useState(settings.org || "FoodsUp-Inc");
  const [accountMsg, setAccountMsg] = useState<string | null>(null);
  const [testingAccountId, setTestingAccountId] = useState<number | null>(null);

  // v0.3.17+：GitHub OAuth Device Flow 登录（零注册，client_id 后端内置默认）。
  const [oauthPhase, setOauthPhase] = useState<"idle" | "code" | "success">("idle");
  const [oauthStart, setOauthStart] = useState<DeviceLoginStart | null>(null);
  const [oauthMsg, setOauthMsg] = useState<string | null>(null);
  const [oauthBusy, setOauthBusy] = useState(false);
  // 轮询取消令牌：面板关闭 / 重新发起时 +1，旧循环检测到变化即退出。
  const oauthRunRef = useRef(0);

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

  // v0.3.16+：删除账号。
  const deleteAccount = async (id: number) => {
    if (!confirm("确认删除该账号？其下的任务仍会保留，但卡片上会标记「账号已删除」。")) {
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
      setAccountMsg(`账号 #${id} 连接正常 · @${res.login}`);
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
      setAccountMsg(`已设为默认账号`);
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
          setOauthMsg(`登录成功 · 已授权 @${res.login}`);
          onAccountsChanged?.();
          return;
        }
        setOauthPhase("idle");
        setOauthMsg(res.message || "登录失败，请重试");
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
        <h3 className="modal-title">设置</h3>

        <div className="field">
          <label>定时同步间隔</label>
          <div className="row">
            <input
              className="input"
              type="number"
              min={5}
              value={minutes}
              onChange={(e) => setMinutes(Math.max(5, Number(e.target.value) || 5))}
            />
            <span className="muted">分钟</span>
          </div>
          <div className="presets">
            {PRESETS.map((p) => (
              <button
                key={p}
                className={`chip${minutes === p ? " on" : ""}`}
                onClick={() => setMinutes(p)}
              >
                {p} 分钟
              </button>
            ))}
          </div>
          <div className="muted small">
            应用常驻菜单栏时按此间隔自动同步；关闭应用后不再触发
          </div>
        </div>

        {/* v0.3.16+：多账号管理（v0.3.17 起登录走 Device Flow）。 */}
        <div className="field">
          <label>GitHub 账号</label>
          {accounts.length === 0 ? (
            <div className="muted small">尚未添加账号</div>
          ) : (
            <div className="account-list">
              {accounts.map((a) => (
                <div key={a.id} className="account-row">
                  <div className="account-info">
                    <div className="account-row-head">
                      <strong>{a.label}</strong>
                      {a.isDefault && (
                        <span className="default-tag" title="默认账号">★ 默认</span>
                      )}
                    </div>
                    <div className="muted small">
                      @{a.login} · {a.org} ·{" "}
                      {a.hasPat ? (
                        "已授权"
                      ) : (
                        <span className="warn">未授权</span>
                      )}
                    </div>
                  </div>
                  <div className="account-row-actions">
                    <button
                      className="btn small"
                      onClick={() => void testAccount(a.id)}
                      disabled={!a.hasPat || testingAccountId === a.id}
                      title="测试该账号的 token 是否仍有效"
                    >
                      {testingAccountId === a.id ? "测试中…" : "测试"}
                    </button>
                    {!a.isDefault && (
                      <button
                        className="btn small"
                        onClick={() => void setDefault(a.id)}
                        title="设为默认账号"
                      >
                        设为默认
                      </button>
                    )}
                    <button
                      className="btn small ghost"
                      onClick={() => void deleteAccount(a.id)}
                      disabled={a.isDefault}
                      title={
                        a.isDefault
                          ? "默认账号不可删除，请先把另一个账号设为默认"
                          : "删除该账号"
                      }
                    >
                      删除
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
              + GitHub 登录添加账号
            </button>
          ) : (
            <div className="account-form">
              {oauthPhase === "idle" && (
                <>
                  <input
                    className="input wide"
                    placeholder="账号名称（可留空，默认用 GitHub login）"
                    value={newLabel}
                    onChange={(e) => setNewLabel(e.target.value)}
                  />
                  <input
                    className="input wide"
                    placeholder="组织（org，如 FoodsUp-Inc）"
                    value={newOrg}
                    onChange={(e) => setNewOrg(e.target.value)}
                  />
                  <div className="row" style={{ marginTop: 6 }}>
                    <button
                      className="btn primary"
                      onClick={() => void runDeviceLogin(newLabel, newOrg)}
                      disabled={oauthBusy}
                    >
                      {oauthBusy ? "正在打开浏览器…" : "通过 GitHub 授权登录"}
                    </button>
                    <button
                      className="btn"
                      onClick={() => {
                        oauthRunRef.current += 1;
                        setAddingAccount(false);
                        setNewLabel("");
                        setNewOrg(settings.org || "FoodsUp-Inc");
                      }}
                    >
                      取消
                    </button>
                  </div>
                  <div className="muted small" style={{ marginTop: 6 }}>
                    点击后自动打开浏览器，登录 GitHub 并点击 Authorize 即完成——无需注册、无需填任何 ID、无需粘贴 token。
                  </div>
                </>
              )}

              {oauthPhase === "code" && oauthStart && (
                <div className="device-flow">
                  <div className="muted small">在浏览器中输入以下一次性代码完成授权：</div>
                  <div className="user-code" title="点击复制">
                    {oauthStart.userCode}
                  </div>
                  <div className="row" style={{ marginTop: 8 }}>
                    <button
                      className="btn"
                      onClick={() => void api.openInBrowser(oauthStart.verificationUriComplete)}
                    >
                      重新打开授权页
                    </button>
                    <button className="btn ghost" onClick={cancelDeviceLogin}>
                      取消登录
                    </button>
                  </div>
                  <div className="muted small" style={{ marginTop: 6 }}>
                    等待授权中…（{oauthStart.expiresIn / 60} 分钟内有效）
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
                      setNewOrg(settings.org || "FoodsUp-Inc");
                    }}
                  >
                    完成
                  </button>
                </div>
              )}

              {oauthMsg && oauthPhase === "idle" && (
                <div className="banner error inline">{oauthMsg}</div>
              )}
            </div>
          )}
          <div className="muted small" style={{ marginTop: 6 }}>
            顶栏下拉切换激活账号；视图模式「全部账号」可聚合查看所有账号任务。
            token 明文存入本地 SQLite——v0.3.17 计划升级为系统 keyring。
          </div>
          {accountMsg && <div className="banner ok inline">{accountMsg}</div>}
        </div>

        <div className="field">
          <label>gh 可执行文件路径</label>
          <input
            className="input wide"
            placeholder="留空（v0.3.15 起仅作展示，已不再使用）"
            value={ghPath}
            onChange={(e) => setGhPath(e.target.value)}
          />
          <div className="muted small">
            历史字段，保留兼容；当前同步完全走账号授权 token，不再探测 gh CLI。
          </div>
        </div>

        <div className="field readonly">
          <label>组织</label>
          <div className="muted">{settings.org}（写于 meta.org，可手动改库）</div>
        </div>

        <div className="field readonly">
          <label>本地数据库</label>
          <div className="muted small path">{settings.dbPath}</div>
        </div>

        {err && <div className="banner error">{err}</div>}

        <div className="modal-actions">
          <button className="btn" onClick={onClose}>
            取消
          </button>
          <button className="btn primary" onClick={save} disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}
