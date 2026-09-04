import type { MouseEvent } from "react";
import type { Task } from "../types";
import { api } from "../api";

interface Props {
  task: Task;
  active: boolean;
  onClick: () => void;
}

// 在浏览器中打开外链：阻止 webview 自身跳转，改用本机默认浏览器打开。
function openExternal(url: string, e: MouseEvent) {
  e.preventDefault();
  e.stopPropagation();
  void api.openInBrowser(url);
}

export default function TaskCard({ task, active, onClick }: Props) {
  const mine = task.ownership === "assigned";
  const assigneeNames = task.assignees
    ? task.assignees.split(",").filter(Boolean)
    : [];

  return (
    <article
      className={`card${active ? " active" : ""}${
        task.candidateDone ? " candidate" : ""
      }${task.ownership === "notassignee" ? " unassigned" : ""}${mine ? " mine" : ""}`}
      onClick={onClick}
    >
      <div className="card-top">
        <span className="repo">{task.repo}</span>
        <span className="num">#{task.number}</span>
        {mine && (
          <span className="mine-badge" title="分配给我">
            ★ 我的
          </span>
        )}
        {task.ghStatus && (() => {
          // v0.3.15+：gh-status-* 配色类按 Status 原文匹配，支持 emoji 与文案变体。
          // 顺序敏感：先匹配「开发中」类以免被「开发完成/测试中」误判为 todo。
          let cls = "gh-status gh-status-default";
          const s = task.ghStatus;
          if (s.includes("测试") || s.includes("待上线")) cls = "gh-status gh-status-processed";
          else if (s.includes("开发中")) cls = "gh-status gh-status-doing";
          else if (s.includes("待开发") || s.includes("需求") || s.includes("规划")) cls = "gh-status gh-status-todo";
          else if (s.includes("取消")) cls = "gh-status gh-status-canceled";
          else if (s.includes("完成") || s.includes("上线")) cls = "gh-status gh-status-done";
          return (
            <span className={cls} title="GitHub Project 状态">
              {s}
            </span>
          );
        })()}
      </div>

      <p className="card-title">{task.title}</p>

      {/* 时间上方一行：分配人 / @我 / 无人认领；分支不再展示在卡片（仅在详情中显示）。 */}
      <div className="meta-row">
        {assigneeNames.length > 0 && (
          <span className="assignee-info">
            <span className="assignee-label">分配人</span>
            <span className="assignee-names">
              {assigneeNames.map((a) => (
                <span key={a} className="assignee-name">
                  @{a}
                </span>
              ))}
            </span>
          </span>
        )}
        {task.mentioned && (
          <span className="mention-badge" title="评论区有人 @我">
            📣 @我
          </span>
        )}
        {/* 仅未认领的任务在此行显示"无人认领"标识。 */}
        {task.ownership === "notassignee" && (
          <span className="unassigned-tag" title="该 issue 暂无负责人">
            无人认领
          </span>
        )}
      </div>

      <div className="card-bottom">
        {task.sessionId ? (
          <span className="session">
            <span className="session-label">会话</span>
            <code>{task.sessionId}</code>
          </span>
        ) : (
          <span className="muted small">
            {task.updatedAt ? task.updatedAt.slice(0, 10) : ""}
          </span>
        )}
        {task.latestCommentUrl && (
          <a
            className="cmt-link"
            title="跳转到最新评论"
            href={task.latestCommentUrl}
            onClick={(e) => openExternal(task.latestCommentUrl, e)}
          >
            💬 新评论
          </a>
        )}
        {task.prNumber > 0 && task.prUrl && (
          <a
            className="pr-link"
            title="对应的 PR"
            href={task.prUrl}
            onClick={(e) => openExternal(task.prUrl, e)}
          >
            🔗 PR #{task.prNumber}
          </a>
        )}
        {task.candidateDone && <span className="candidate-tag">待确认关闭</span>}
      </div>
    </article>
  );
}
