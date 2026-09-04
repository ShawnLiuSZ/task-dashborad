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
  const assigneeNames = task.assignees ? task.assignees.split(",").filter(Boolean) : [];

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
        {task.ghStatus && (
          <span className="gh-status" title="GitHub Project 状态">
            {task.ghStatus}
          </span>
        )}
      </div>

      <p className="card-title">{task.title}</p>

      {/* 时间上方一行：@我 / 分配人(无人认领) / 关联分支，统一收在此行，不再挤在标题行。 */}
      <div className="meta-row">
        {task.mentioned && (
          <span className="mention-badge" title="评论区有人 @我">
            📣 @我
          </span>
        )}
        {assigneeNames.length > 0 ? (
          <span className="assignee-info">
            <span className="assignee-label">分配人</span>
            <span className="assignee-names">
              {assigneeNames.map((a, i) => (
                <span key={i} className="assignee-name">
                  @{a}
                </span>
              ))}
            </span>
          </span>
        ) : (
          <span className="unassigned-tag" title="该 issue 暂无负责人">
            无人认领
          </span>
        )}
        {task.branch && (
          <span className="branch-tag" title="关联 PR 的分支">
            🌿 {task.branch}
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
