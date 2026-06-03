import { useEffect, useState } from 'react'
import { api } from '../api'
import type { CronJob, SettingsView, SkillFull, Usage } from '../types'

type Tab = 'cron' | 'skills' | 'memory' | 'usage' | 'settings'
const TABS: Tab[] = ['cron', 'skills', 'memory', 'usage', 'settings']

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
          {tab === 'settings' && <SettingsTab />}
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

function SettingsTab() {
  const [s, setS] = useState<SettingsView | null>(null)
  const [saved, setSaved] = useState(false)
  const [voice, setVoice] = useState({
    enabled: false,
    stt_base_url: '',
    stt_model: '',
    tts_provider: 'openai',
    tts_base_url: '',
    tts_model: '',
    tts_voice: '',
  })
  const [gw, setGw] = useState({ yolo: false, whatsapp_phone_number_id: '', whatsapp_verify_token: '' })
  const blankSecrets = {
    voice_api_key: '',
    tts_api_key: '',
    telegram_token: '',
    discord_token: '',
    slack_bot_token: '',
    slack_app_token: '',
    whatsapp_token: '',
  }
  const [secrets, setSecrets] = useState({ ...blankSecrets })

  function load(d: SettingsView) {
    setS(d)
    setVoice({
      enabled: d.voice.enabled,
      stt_base_url: d.voice.stt_base_url,
      stt_model: d.voice.stt_model,
      tts_provider: d.voice.tts_provider || 'openai',
      tts_base_url: d.voice.tts_base_url,
      tts_model: d.voice.tts_model,
      tts_voice: d.voice.tts_voice,
    })
    setGw({
      yolo: d.gateway.yolo,
      whatsapp_phone_number_id: d.gateway.whatsapp_phone_number_id,
      whatsapp_verify_token: d.gateway.whatsapp_verify_token,
    })
  }
  useEffect(() => {
    api.settingsGet().then(load).catch(() => {})
  }, [])
  if (!s) return <div className="cc-empty">loading…</div>

  async function save() {
    const patch: Record<string, unknown> = {
      voice_enabled: voice.enabled,
      stt_base_url: voice.stt_base_url,
      stt_model: voice.stt_model,
      tts_provider: voice.tts_provider,
      tts_base_url: voice.tts_base_url,
      tts_model: voice.tts_model,
      tts_voice: voice.tts_voice,
      gateway_yolo: gw.yolo,
      whatsapp_phone_number_id: gw.whatsapp_phone_number_id,
      whatsapp_verify_token: gw.whatsapp_verify_token,
    }
    for (const [k, val] of Object.entries(secrets)) if (val) patch[k] = val
    await api.settingsSet(patch)
    setSecrets({ ...blankSecrets })
    setSaved(true)
    setTimeout(() => setSaved(false), 1500)
    api.settingsGet().then(load).catch(() => {})
  }

  const ph = (set: boolean) => (set ? '•••••••• (set — type to replace)' : 'not set')
  const setSecret = (k: keyof typeof blankSecrets, v: string) => setSecrets({ ...secrets, [k]: v })

  return (
    <div className="cc-pane">
      <div className="cc-section">Voice</div>
      <label className="cc-check">
        <input
          type="checkbox"
          checked={voice.enabled}
          onChange={(e) => setVoice({ ...voice, enabled: e.target.checked })}
        />{' '}
        enabled
      </label>
      <Field label="STT base URL" value={voice.stt_base_url} onChange={(x) => setVoice({ ...voice, stt_base_url: x })} />
      <Field label="STT model" value={voice.stt_model} onChange={(x) => setVoice({ ...voice, stt_model: x })} />
      <Secret label="STT API key" placeholder={ph(s.voice.api_key_set)} value={secrets.voice_api_key} onChange={(x) => setSecret('voice_api_key', x)} />
      <label className="cc-field">
        <span>TTS provider</span>
        <select value={voice.tts_provider} onChange={(e) => setVoice({ ...voice, tts_provider: e.target.value })}>
          <option value="openai">OpenAI-compatible</option>
          <option value="elevenlabs">ElevenLabs</option>
        </select>
      </label>
      {voice.tts_provider === 'elevenlabs' ? (
        <>
          <Field label="ElevenLabs model" value={voice.tts_model} onChange={(x) => setVoice({ ...voice, tts_model: x })} />
          <Field label="ElevenLabs voice id" value={voice.tts_voice} onChange={(x) => setVoice({ ...voice, tts_voice: x })} />
          <Secret label="ElevenLabs API key" placeholder={ph(s.voice.tts_api_key_set)} value={secrets.tts_api_key} onChange={(x) => setSecret('tts_api_key', x)} />
        </>
      ) : (
        <>
          <Field label="TTS base URL" value={voice.tts_base_url} onChange={(x) => setVoice({ ...voice, tts_base_url: x })} />
          <Field label="TTS model" value={voice.tts_model} onChange={(x) => setVoice({ ...voice, tts_model: x })} />
          <Field label="TTS voice" value={voice.tts_voice} onChange={(x) => setVoice({ ...voice, tts_voice: x })} />
          <Secret label="TTS API key" placeholder={ph(s.voice.tts_api_key_set)} value={secrets.tts_api_key} onChange={(x) => setSecret('tts_api_key', x)} />
        </>
      )}

      <div className="cc-section">Gateways</div>
      <label className="cc-check">
        <input type="checkbox" checked={gw.yolo} onChange={(e) => setGw({ ...gw, yolo: e.target.checked })} /> auto-approve
        tool calls (sandbox recommended)
      </label>
      <Secret label="Telegram token" placeholder={ph(s.gateway.telegram_token_set)} value={secrets.telegram_token} onChange={(x) => setSecret('telegram_token', x)} />
      <Secret label="Discord token" placeholder={ph(s.gateway.discord_token_set)} value={secrets.discord_token} onChange={(x) => setSecret('discord_token', x)} />
      <Secret label="Slack bot token" placeholder={ph(s.gateway.slack_bot_token_set)} value={secrets.slack_bot_token} onChange={(x) => setSecret('slack_bot_token', x)} />
      <Secret label="Slack app token" placeholder={ph(s.gateway.slack_app_token_set)} value={secrets.slack_app_token} onChange={(x) => setSecret('slack_app_token', x)} />
      <Secret label="WhatsApp token" placeholder={ph(s.gateway.whatsapp_token_set)} value={secrets.whatsapp_token} onChange={(x) => setSecret('whatsapp_token', x)} />
      <Field label="WhatsApp phone_number_id" value={gw.whatsapp_phone_number_id} onChange={(x) => setGw({ ...gw, whatsapp_phone_number_id: x })} />
      <Field label="WhatsApp verify token" value={gw.whatsapp_verify_token} onChange={(x) => setGw({ ...gw, whatsapp_verify_token: x })} />

      <button className="cc-save" onClick={save}>
        {saved ? 'saved ✓' : 'Save settings'}
      </button>
      <div className="cc-dim">
        Voice changes apply immediately. Gateway changes apply when you start <code>blumi gateway</code>.
      </div>
    </div>
  )
}

function Field({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  return (
    <label className="cc-field">
      <span>{label}</span>
      <input value={value} onChange={(e) => onChange(e.target.value)} />
    </label>
  )
}

function Secret({
  label,
  placeholder,
  value,
  onChange,
}: {
  label: string
  placeholder: string
  value: string
  onChange: (v: string) => void
}) {
  return (
    <label className="cc-field">
      <span>{label}</span>
      <input type="password" placeholder={placeholder} value={value} onChange={(e) => onChange(e.target.value)} />
    </label>
  )
}
