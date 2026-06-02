import type { Todo } from '../types'

type Props = {
  todos: Todo[]
  usage: { input: number; output: number }
  model: string
  persona: string
  busy: boolean
  contextTokens: number
  contextSize: number
  uptimeSecs: number
  activeSecs: number
}

function fmtDur(secs: number): string {
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (d > 0) return `${d}d ${h}h`
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

function fmtK(n: number): string {
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n)
}

export function RunPanel({
  todos,
  usage,
  model,
  persona,
  busy,
  contextTokens,
  contextSize,
  uptimeSecs,
  activeSecs,
}: Props) {
  const total = todos.length
  const done = todos.filter((t) => t.status === 'completed').length
  const active = todos.filter((t) => t.status === 'in_progress').length
  const pending = total - done - active
  const taskPct = total ? Math.round((done / total) * 100) : 0

  const ctxFrac = contextSize > 0 ? Math.min(1, contextTokens / contextSize) : 0
  const ctxPct = Math.round(ctxFrac * 100)
  const ctxClass = ctxFrac > 0.85 ? 'danger' : ctxFrac > 0.6 ? 'warn' : 'ok'

  return (
    <aside className="run">
      <div className="section">Session</div>
      <div className="kv">
        <span>status</span>
        <b className={busy ? 'on' : ''}>{busy ? 'working' : 'ready'}</b>
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
        <span>uptime</span>
        <b>{fmtDur(uptimeSecs)}</b>
      </div>
      <div className="kv">
        <span>active</span>
        <b>{fmtDur(activeSecs)}</b>
      </div>

      <div className="section">Context</div>
      <div className="progress" title={`${ctxPct}% of context window`}>
        <div className={`progress-fill ${ctxClass}`} style={{ width: `${ctxPct}%` }} />
      </div>
      <div className="muted small">
        {fmtK(contextTokens)} / {fmtK(contextSize)} ({ctxPct}%)
      </div>

      <div className="section">Usage</div>
      <div className="kv">
        <span>tokens</span>
        <b>
          ↑{fmtK(usage.input)} ↓{fmtK(usage.output)}
        </b>
      </div>

      <div className="section">
        Tasks {done}/{total}
      </div>
      {total === 0 ? (
        <div className="muted">no tasks yet</div>
      ) : (
        <>
          <div className="progress" title={`${taskPct}% complete`}>
            <div className="progress-fill" style={{ width: `${taskPct}%` }} />
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
