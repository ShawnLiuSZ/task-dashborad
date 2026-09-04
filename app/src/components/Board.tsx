import { COLUMNS, type Account, type StatusKey, type Task } from "../types";
import TaskCard from "./TaskCard";

interface Props {
  tasks: Task[];
  selected: string | null;
  onSelect: (key: string) => void;
  /** v0.3.16+：账号列表（按 id），用于在卡片上显示账号徽章；空 Map 时不显示徽章。 */
  accounts?: Map<number, Account>;
}

export default function Board({ tasks, selected, onSelect, accounts }: Props) {
  const byStatus = (s: StatusKey) => tasks.filter((t) => t.status === s);

  return (
    <div className="board">
      {COLUMNS.map((col) => {
        const items = byStatus(col.key);
        return (
          <section key={col.key} className={`column column-${col.key}`}>
            <div className="column-head">
              <span className={`dot dot-${col.key}`} />
              <span className="column-title">{col.label}</span>
              <span className="count">{items.length}</span>
            </div>
            <div className="column-body">
              {items.length === 0 && <div className="empty">{col.hint}</div>}
              {items.map((t) => (
                <TaskCard
                  key={t.key}
                  task={t}
                  accountLabel={accounts?.get(t.accountId)?.label}
                  active={t.key === selected}
                  onClick={() => onSelect(t.key)}
                />
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}
