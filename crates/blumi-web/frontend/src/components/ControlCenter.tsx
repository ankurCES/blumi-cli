import { useEffect, useState } from 'react'
import { api } from '../api'
import type { CronJob, SkillFull, Usage } from '../types'

type Tab = 'cron' | 'skills' | 'memory' | 'usage'
const TABS: Tab[] = ['cron', 'skills', 'memory', 'usage']

export function ControlCenter({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<Tab>('cron')
  return (
    <div className="cc-overlay" onClick={onClose}>
      <div className="cc-modal" onClick={(e) => e.stopPropagation()}>
        <div className="cc-head">
          <span className="cc-title">Control Center</span>
          <button className="cc-x" onClick={onClose}>
            ✕
          </button>
        </div>
        <div className="cc-tabs">
          {TABS.map((t) => (
            <button
              key={t}
              className={`cc-tab ${tab === t ? 'active' : ''}`}
              onClick={() => setTab(t)}
            >
              {t}
            </button>
          ))}
        </div>
        <div className="cc-body">
          {tab === 'cron' && <CronTab />}
          {tab === 'skills' && <SkillsTab />}
          {tab === 'memory' && <MemoryTab />}
          {tab === 'usage' && <UsageTab />}
        </div>
      </div>
    </div>
  )
}

function CronTab() {
  const [jobs, setJobs] = useState<CronJob[]>([])
  const [name, setName] = useState('')
  const [schedule, setSchedule] = useState('')
  const [prompt, setPrompt] = useState('')
  const [err, setErr] = useState('')

  const load = () => api.cron().then(setJobs).catch(() => {})
  useEffect(() => {
    load()
  }, [])

  async function add(e: React.FormEvent) {
    e.preventDefault()
    setErr('')
    const r = (await api.cronAdd(name, schedule, prompt)) as { ok?: boolean; error?: string }
    if (r.ok) {
      setName('')
      setSchedule('')
      setPrompt('')
      load()
    } else setErr(r.error || 'failed')
  }
  async function remove(id: string) {
    await api.cronRemove(id)
    load()
  }

  return (
    <div className="cc-pane">
      {jobs.length === 0 && <div className="cc-empty">no scheduled jobs</div>}
      {jobs.map((j) => (
        <div className="cc-row" key={j.id}>
          <div className="cc-row-main">
            <strong>{j.name}</strong> <span className="cc-dim">{j.schedule}</span>
            <div className="cc-dim cc-clip">{j.prompt}</div>
          </div>
          <button className="cc-del" onClick={() => remove(j.id)}>
            remove
          </button>
        </div>
      ))}
      <form className="cc-form" onSubmit={add}>
        <input placeholder="name" value={name} onChange={(e) => setName(e.target.value)} />
        <input
          placeholder="schedule (e.g. every 1h, daily 09:00)"
          value={schedule}
          onChange={(e) => setSchedule(e.target.value)}
        />
        <input placeholder="prompt" value={prompt} onChange={(e) => setPrompt(e.target.value)} />
        <button type="submit" disabled={!name || !schedule || !prompt}>
          + add job
        </button>
        {err && <span className="cc-err">{err}</span>}
      </form>
    </div>
  )
}

function SkillsTab() {
  const [skills, setSkills] = useState<SkillFull[]>([])
  const [open, setOpen] = useState<string | null>(null)
  useEffect(() => {
    api.skillsList().then(setSkills).catch(() => {})
  }, [])
  return (
    <div className="cc-pane">
      {skills.length === 0 && <div className="cc-empty">no skills yet</div>}
      {skills.map((s) => (
        <div className="cc-row cc-col" key={s.name}>
          <button className="cc-row-main cc-link" onClick={() => setOpen(open === s.name ? null : s.name)}>
            <strong>{s.name}</strong>
            <div className="cc-dim">{s.description}</div>
          </button>
          {open === s.name && <pre className="cc-pre">{s.body}</pre>}
        </div>
      ))}
    </div>
  )
}

function MemoryTab() {
  const [memory, setMemory] = useState('')
  const [user, setUser] = useState('')
  const [saved, setSaved] = useState('')
  useEffect(() => {
    api.memoryGet().then((m) => {
      setMemory(m.memory)
      setUser(m.user)
    }).catch(() => {})
  }, [])
  async function save(which: 'memory' | 'user') {
    await api.memorySet(which, which === 'memory' ? memory : user)
    setSaved(which)
    setTimeout(() => setSaved(''), 1500)
  }
  return (
    <div className="cc-pane">
      <label className="cc-label">MEMORY.md (agent notes)</label>
      <textarea className="cc-area" value={memory} onChange={(e) => setMemory(e.target.value)} />
      <button className="cc-save" onClick={() => save('memory')}>
        {saved === 'memory' ? 'saved ✓' : 'save'}
      </button>
      <label className="cc-label">USER.md (about you)</label>
      <textarea className="cc-area" value={user} onChange={(e) => setUser(e.target.value)} />
      <button className="cc-save" onClick={() => save('user')}>
        {saved === 'user' ? 'saved ✓' : 'save'}
      </button>
    </div>
  )
}

function UsageTab() {
  const [u, setU] = useState<Usage | null>(null)
  useEffect(() => {
    api.usage().then(setU).catch(() => {})
  }, [])
  if (!u) return <div className="cc-empty">loading…</div>
  const k = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`)
  return (
    <div className="cc-pane">
      <div className="cc-stats">
        <Stat label="sessions" value={`${u.sessions}`} />
        <Stat label="messages" value={`${u.messages}`} />
        <Stat label="input tok" value={k(u.input_tokens)} />
        <Stat label="output tok" value={k(u.output_tokens)} />
      </div>
      <label className="cc-label">By model</label>
      <table className="cc-table">
        <thead>
          <tr>
            <th>model</th>
            <th>sessions</th>
            <th>in</th>
            <th>out</th>
          </tr>
        </thead>
        <tbody>
          {u.by_model.map((m) => (
            <tr key={m.model}>
              <td>{m.model}</td>
              <td>{m.sessions}</td>
              <td>{k(m.input_tokens)}</td>
              <td>{k(m.output_tokens)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="cc-stat">
      <div className="cc-stat-val">{value}</div>
      <div className="cc-stat-lbl">{label}</div>
    </div>
  )
}
