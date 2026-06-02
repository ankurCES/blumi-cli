import type { Config, Persona } from '../types'

type Props = {
  config: Config | null
  connected: boolean
  personas: Persona[]
  persona: string
  onPersona: (name: string) => void
  yolo: boolean
  onYolo: (on: boolean) => void
  busy: boolean
  onCompact: () => void
  onUndo: () => void
  onCenter: () => void
}

export function Header({
  config,
  connected,
  personas,
  persona,
  onPersona,
  yolo,
  onYolo,
  busy,
  onCompact,
  onUndo,
  onCenter,
}: Props) {
  return (
    <header className="header">
      <div className="brand">
        <span className="flower">✿</span>
        <span className="wordmark">blumi</span>
      </div>
      <div className="header-meta">
        {personas.length > 0 && (
          <label className="picker" title="Agent persona">
            <span className="picker-label">persona</span>
            <select value={persona} onChange={(e) => onPersona(e.target.value)}>
              {personas.map((p) => (
                <option key={p.name} value={p.name} title={p.description}>
                  {p.name}
                </option>
              ))}
            </select>
          </label>
        )}
        {config && (
          <span className="model" title="Active model">
            {config.model || 'default'}
          </span>
        )}
        <button
          className={`yolo ${yolo ? 'on' : ''}`}
          onClick={() => onYolo(!yolo)}
          title="Auto-approve tool calls without prompting"
        >
          {yolo ? '● auto-approve' : '○ auto-approve'}
        </button>
        <button className="hbtn" onClick={onCompact} disabled={busy} title="Compact the context now">
          compact
        </button>
        <button className="hbtn" onClick={onUndo} disabled={busy} title="Undo the last file change">
          undo
        </button>
        <button className="hbtn" onClick={onCenter} title="Control center: cron, skills, memory, usage">
          ⚙ center
        </button>
        {config && (
          <span className="cwd" title={config.working_dir}>
            {shorten(config.working_dir)}
          </span>
        )}
        <span
          className={`dot ${connected ? 'live' : 'dead'}`}
          title={connected ? 'connected' : 'disconnected'}
        />
      </div>
    </header>
  )
}

function shorten(p: string): string {
  const parts = p.split('/')
  return parts.length > 3 ? '…/' + parts.slice(-2).join('/') : p
}
