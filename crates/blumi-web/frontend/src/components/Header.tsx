import { useState } from 'react'
import type { Config, ModelOptions, Persona } from '../types'

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
  models: ModelOptions | null
  onProvider: (name: string, key?: string) => void
  onModel: (model: string) => void
  onReload: () => void
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
  models,
  onProvider,
  onModel,
  onReload,
}: Props) {
  // When an unready provider is picked, prompt for its key inline.
  const [pendingKey, setPendingKey] = useState<string | null>(null)
  const [keyInput, setKeyInput] = useState('')
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
        {models && models.providers.length > 0 && (
          <label className="picker" title="LLM provider (switching reloads the agent)">
            <span className="picker-label">provider</span>
            <select
              value={models.provider}
              onChange={(e) => {
                const p = models.providers.find((x) => x.name === e.target.value)
                if (p && !p.ready) {
                  setPendingKey(p.name)
                  setKeyInput('')
                } else {
                  onProvider(e.target.value)
                }
              }}
            >
              {models.providers.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.label}
                  {p.ready ? '' : ' (add key)'}
                </option>
              ))}
            </select>
          </label>
        )}
        {pendingKey && (
          <form
            className="keyform"
            onSubmit={(e) => {
              e.preventDefault()
              onProvider(pendingKey, keyInput)
              setPendingKey(null)
            }}
          >
            <input
              type="password"
              autoFocus
              placeholder={`${pendingKey} API key`}
              value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)}
            />
            <button className="hbtn" type="submit" disabled={!keyInput.trim()}>
              connect
            </button>
            <button className="hbtn" type="button" onClick={() => setPendingKey(null)}>
              cancel
            </button>
          </form>
        )}
        {models && (
          <label className="picker" title="Active model">
            <span className="picker-label">model</span>
            <select value={models.model} onChange={(e) => onModel(e.target.value)}>
              {(models.models.length ? models.models : [models.model || 'default']).map((m) => (
                <option key={m} value={m}>
                  {m || 'default'}
                </option>
              ))}
            </select>
          </label>
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
        <button
          className="hbtn"
          onClick={onReload}
          disabled={busy}
          title="Reload the agent (re-read config, skills, memory) — keeps the conversation"
        >
          ↻ reload
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
