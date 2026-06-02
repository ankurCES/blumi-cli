import type { SessionMeta } from '../types'

type Props = {
  sessions: SessionMeta[]
  onNew: () => void
  onResume: (id: string) => void
}

export function Sidebar({ sessions, onNew, onResume }: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span>Sessions</span>
        <button className="btn ghost" onClick={onNew} title="Start a fresh session">
          + new
        </button>
      </div>
      <div className="session-list">
        {sessions.length === 0 && <div className="muted">no past sessions</div>}
        {sessions.map((s) => (
          <button
            className="session-item"
            key={s.id}
            title={`resume ${s.id}`}
            onClick={() => onResume(s.id)}
          >
            <div className="session-title">{s.title || '(untitled)'}</div>
            <div className="session-meta">
              {s.model || 'default'} · {s.message_count} msgs
            </div>
          </button>
        ))}
      </div>
    </aside>
  )
}
