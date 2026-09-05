import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { CheckUpdate } from "../types";

interface Props {
  onClose: () => void;
}

type State =
  | { phase: "loading" }
  | { phase: "ok"; data: CheckUpdate }
  | { phase: "error"; message: string };

/** v0.3.19+「关于」弹窗：展示当前版本号 + 检查更新入口。 */
export default function AboutPanel({ onClose }: Props) {
  const { t } = useI18n();
  const [version, setVersion] = useState<string>("");
  const [state, setState] = useState<State>({ phase: "loading" });
  const [checkedOnce, setCheckedOnce] = useState(false);

  const loadVersion = useCallback(async () => {
    try {
      setVersion(await api.getAppVersion());
    } catch (e) {
      setVersion("?");
    }
  }, []);

  const check = useCallback(async () => {
    setState({ phase: "loading" });
    setCheckedOnce(true);
    try {
      const d = await api.checkLatestRelease();
      if (d.error) {
        setState({ phase: "error", message: d.error });
      } else {
        setState({ phase: "ok", data: d });
      }
    } catch (e) {
      setState({ phase: "error", message: String(e) });
    }
  }, []);

  // 打开弹窗时先拉一次当前版本（不自动联网检查）。
  useEffect(() => {
    void loadVersion();
  }, [loadVersion]);

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal about-modal" onClick={(e) => e.stopPropagation()}>
        <h3 className="modal-title">{t("about.title")}</h3>

        <div className="about-body">
          <div className="field readonly">
            <label>{t("about.versionLabel")}</label>
            <div className="muted small">v{version}</div>
          </div>

          <div className="muted small" style={{ marginTop: 6 }}>
            {t("about.localData")}
          </div>
          <div className="about-repo-row" style={{ marginTop: 4 }}>
            <span className="muted small">{t("about.repoPath")}</span>
            <button
              className="about-repo-link"
              title="https://github.com/ShawnLiuSZ/task-dashborad"
              onClick={() => void api.openInBrowser("https://github.com/ShawnLiuSZ/task-dashborad")}
            >
              ShawnLiuSZ/task-dashborad ↗
            </button>
          </div>

          {checkedOnce && state.phase === "loading" && (
            <div className="about-status">{t("about.checking")}</div>
          )}
          {state.phase === "ok" &&
            (state.data.upToDate ? (
              <div className="about-status up-to-date">
                {"✅"} {t("about.upToDate")}
              </div>
            ) : (
              <div className="about-status has-update">
                {"✨"} {t("about.updateAvailable", { latest: state.data.latest, current: state.data.current })}
                {state.data.url && (
                  <button
                    className="btn primary"
                    style={{ marginTop: 6 }}
                    onClick={() => void api.openInBrowser(state.data.url)}
                  >
                    {t("about.download")} ↗
                  </button>
                )}
              </div>
            ))}
          {state.phase === "error" && (
            <div className="about-status error">
              {t("about.error", { error: state.message })}
            </div>
          )}
        </div>

        <div className="modal-actions">
          <button className="btn" onClick={onClose}>
            {t("btn.close")}
          </button>
          <button className="btn primary" onClick={check} disabled={state.phase === "loading"}>
            {state.phase === "loading" && checkedOnce ? t("about.checking") : t("about.checkUpdate")}
          </button>
        </div>
      </div>
    </div>
  );
}