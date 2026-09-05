import { useState } from "react";
import { api } from "../api";
import { useI18n, type LangMode } from "../i18n";
import type { Settings } from "../types";

interface Props {
  settings: Settings;
  onSaved: (s: Settings) => void;
  onClose: () => void;
}

const PRESETS = [15, 30, 60, 120, 240];

export default function SettingsPanel({
  settings,
  onSaved,
  onClose,
}: Props) {
  const { t, mode, setMode } = useI18n();
  const [minutes, setMinutes] = useState(settings.scheduleMinutes);
  const [ghPath, setGhPath] = useState(settings.ghPath);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [diagBusy, setDiagBusy] = useState(false);
  const [diagMsg, setDiagMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [diagAccountId, setDiagAccountId] = useState<number | null>(
    settings.accounts.find((a) => a.isDefault)?.id ?? settings.accounts[0]?.id ?? null,
  );
  const [projects, setProjects] = useState<any[]>([]);

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

  const loadProjects = async (accountId: number) => {
    try {
      setProjects(await api.listProjects(accountId));
    } catch {
      setProjects([]);
    }
  };

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3 className="modal-title">{t("settings.title")}</h3>

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
              {settings.accounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.label}{a.isDefault ? " ★" : ""}
                </option>
              ))}
            </select>
            <button
              className="btn"
              onClick={() => { void diagnoseProject(); if (diagAccountId) void loadProjects(diagAccountId); }}
              disabled={diagBusy || settings.accounts.length === 0}
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
