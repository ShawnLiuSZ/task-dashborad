import { useState } from "react";
import { api } from "../api";
import type { Settings } from "../types";

interface Props {
  settings: Settings;
  onSaved: (s: Settings) => void;
  onClose: () => void;
}

const PRESETS = [15, 30, 60, 120, 240];

export default function SettingsPanel({ settings, onSaved, onClose }: Props) {
  const [minutes, setMinutes] = useState(settings.scheduleMinutes);
  const [ghPath, setGhPath] = useState(settings.ghPath);
  const [pat, setPat] = useState("");
  const [hasPat, setHasPat] = useState(settings.hasPat);
  const [patLogin, setPatLogin] = useState(settings.login);
  const [testing, setTesting] = useState(false);
  const [savingPat, setSavingPat] = useState(false);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [patMsg, setPatMsg] = useState<string | null>(null);

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

  // 保存 PAT：立即调 `save_pat`，后端做 token 有效性探测并写 login。
  const savePat = async () => {
    if (!pat.trim()) {
      setPatMsg("请先粘贴 GitHub PAT");
      return;
    }
    setSavingPat(true);
    setPatMsg(null);
    setErr(null);
    try {
      const res = await api.savePat(pat.trim());
      setHasPat(res.hasPat);
      setPatLogin(res.login);
      setPat(""); // 安全：保存后清掉 input 显示，避免截屏或肩膀偷看泄漏
      setPatMsg(`已保存，账号 @${res.login}`);
    } catch (e) {
      setPatMsg(null);
      setErr(String(e));
    } finally {
      setSavingPat(false);
    }
  };

  // 测试当前已保存的 PAT：仅探测，不改 db。
  const testPat = async () => {
    setTesting(true);
    setPatMsg(null);
    setErr(null);
    try {
      const res = await api.testPat();
      setPatLogin(res.login);
      setPatMsg(`连接正常 · 账号 @${res.login}`);
    } catch (e) {
      setPatMsg(null);
      setErr(String(e));
    } finally {
      setTesting(false);
    }
  };

  // 清除 PAT。
  const clearPat = async () => {
    setSavingPat(true);
    setPatMsg(null);
    setErr(null);
    try {
      await api.clearPat();
      setHasPat(false);
      setPatLogin("");
      setPat("");
      setPatMsg("已清除");
    } catch (e) {
      setPatMsg(null);
      setErr(String(e));
    } finally {
      setSavingPat(false);
    }
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

        <div className="field">
          <label>GitHub Personal Access Token</label>
          <input
            className="input wide"
            type="password"
            autoComplete="off"
            spellCheck={false}
            placeholder={
              hasPat ? "已保存（输入新值以覆盖）" : "粘贴 GitHub PAT（repo + read:org + project）"
            }
            value={pat}
            onChange={(e) => setPat(e.target.value)}
          />
          <div className="pat-status">
            <span className="muted">
              当前账号：
              {patLogin ? (
                <strong> @{patLogin}</strong>
              ) : hasPat ? (
                <span> 已配置（未探测）</span>
              ) : (
                <span className="warn">未配置（同步将跳过）</span>
              )}
            </span>
          </div>
          <div className="pat-actions">
            <button
              className="btn primary"
              onClick={savePat}
              disabled={savingPat || !pat.trim()}
            >
              {savingPat ? "保存中…" : "保存 PAT"}
            </button>
            <button className="btn" onClick={testPat} disabled={testing || !hasPat}>
              {testing ? "测试中…" : "测试连接"}
            </button>
            <button className="btn ghost" onClick={clearPat} disabled={!hasPat}>
              清除
            </button>
          </div>
          <div className="muted small">
            推荐使用 <code>fine-grained PAT</code>，仅授予目标组织，读权限即可。
            PAT 明文存入本地 SQLite——v0.3.16 计划升级为系统 keyring。
          </div>
          {patMsg && <div className="banner ok inline">{patMsg}</div>}
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
            历史字段，保留兼容；当前同步完全走上方 PAT，不再探测 gh CLI。
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
