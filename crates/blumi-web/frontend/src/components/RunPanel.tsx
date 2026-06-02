import type { Todo } from '../types'

type Props = {
  todos: Todo[]
  usage: { input: number; output: number }
  model: string
  busy: boolean
}

export function RunPanel({ todos, usage, model, busy }: Props) {
  const done = todos.filter((t) => t.status === 'completed').length
  return (
    <aside className="run">
      <div className="section">Session</div>
      <div className="kv">
        <span>status</span>
        <b className={busy ? 'on' : ''}>{busy ? 'working' : 'idle'}</b>
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
        Tasks {done}/{todos.length}
      </div>
      {todos.length === 0 && <div className="muted">no tasks yet</div>}
      {todos.map((t) => (
        <div className={`todo ${t.status}`} key={t.id}>
          <span className="todo-mark">
            {t.status === 'completed' ? '✓' : t.status === 'in_progress' ? '→' : '•'}
          </span>
          <span className="todo-text">{t.content}</span>
        </div>
      ))}
    </aside>
  )
}
