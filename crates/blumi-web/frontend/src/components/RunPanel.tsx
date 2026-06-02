import type { Todo } from '../types'

type Props = {
  todos: Todo[]
  usage: { input: number; output: number }
  model: string
  persona: string
  busy: boolean
}

export function RunPanel({ todos, usage, model, persona, busy }: Props) {
  const total = todos.length
  const done = todos.filter((t) => t.status === 'completed').length
  const active = todos.filter((t) => t.status === 'in_progress').length
  const pending = total - done - active
  const pct = total ? Math.round((done / total) * 100) : 0

  return (
    <aside className="run">
      <div className="section">Session</div>
      <div className="kv">
        <span>status</span>
        <b className={busy ? 'on' : ''}>{busy ? 'working' : 'idle'}</b>
      </div>
      <div className="kv">
        <span>persona</span>
        <b>{persona || 'default'}</b>
      </div>
      <div className="kv">
        <span>model</span>
        <b>{model || 'default'}</b>
      </div>
      <div className="kv">
        <span>tokens</span>
        <b>
          ↑{usage.input} ↓{usage.output}
        </b>
      </div>

      <div className="section">
        Tasks {done}/{total}
      </div>
      {total === 0 ? (
        <div className="muted">no tasks yet</div>
      ) : (
        <>
          <div className="progress" title={`${pct}% complete`}>
            <div className="progress-fill" style={{ width: `${pct}%` }} />
          </div>
          <div className="task-counts">
            <span className="tc done">{done} done</span>
            <span className="tc active">{active} active</span>
            <span className="tc pending">{pending} pending</span>
          </div>
          <div className="task-list">
            {todos.map((t) => (
              <div className={`todo ${t.status}`} key={t.id}>
                <span className="todo-mark">
                  {t.status === 'completed' ? '✓' : t.status === 'in_progress' ? '→' : '•'}
                </span>
                <span className="todo-text">{t.content}</span>
              </div>
            ))}
          </div>
        </>
      )}
    </aside>
  )
}
