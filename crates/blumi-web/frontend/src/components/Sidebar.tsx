import type { SessionMeta } from '../types'

type Props = {
  sessions: SessionMeta[]
  onNew: () => void
}

export function Sidebar({ sessions, onNew }: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span>Sessions</span>
        <button className="btn ghost" onClick={onNew} title="Clear the current view">
          + new
        </button>
      </div>
      <div className="session-list">
        {sessions.length === 0 && <div className="muted">no past sessions</div>}
        {sessions.map((s) => (
          <div className="session-item" key={s.id} title={s.id}>
            <div className="session-title">{s.title || '(untitled)'}</div>
            <div className="session-meta">
              {s.model} · {s.message_count} msgs
            </div>
          </div>
        ))}
      </div>
    </aside>
  )
}
