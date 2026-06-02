import type { Config } from '../types'

type Props = {
  config: Config | null
  connected: boolean
}

export function Header({ config, connected }: Props) {
  return (
    <header className="header">
      <div className="brand">
        <span className="flower">✿</span>
        <span className="wordmark">blumi</span>
      </div>
      <div className="header-meta">
        {config && <span className="model">{config.model}</span>}
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
