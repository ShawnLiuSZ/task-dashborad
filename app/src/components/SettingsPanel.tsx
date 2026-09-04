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
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

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
          <label>gh 可执行文件路径</label>
          <input
            className="input wide"
            placeholder="留空则自动探测（/opt/homebrew/bin/gh 等）"
            value={ghPath}
            onChange={(e) => setGhPath(e.target.value)}
          />
        </div>

        <div className="field readonly">
          <label>账号</label>
          <div className="muted">
            {settings.login || "未登录"} @ {settings.org}
          </div>
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
