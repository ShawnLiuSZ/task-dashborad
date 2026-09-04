import { COLUMNS, type StatusKey, type Task } from "../types";
import TaskCard from "./TaskCard";

interface Props {
  tasks: Task[];
  selected: string | null;
  onSelect: (key: string) => void;
}

export default function Board({ tasks, selected, onSelect }: Props) {
  const byStatus = (s: StatusKey) => tasks.filter((t) => t.status === s);

  return (
    <div className="board">
      {COLUMNS.map((col) => {
        const items = byStatus(col.key);
        return (
          <section key={col.key} className="column">
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
