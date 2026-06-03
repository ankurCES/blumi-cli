import type {
  Config,
  CronJob,
  ModelOptions,
  Persona,
  ServerMessage,
  SessionMeta,
  SettingsView,
  SkillFull,
  Usage,
} from './types'

async function getJSON<T>(path: string): Promise<T> {
  const r = await fetch(path)
  if (!r.ok) throw new Error(`${path}: ${r.status}`)
  return r.json() as Promise<T>
}

async function postJSON(path: string, body?: unknown): Promise<unknown> {
  const r = await fetch(path, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  })
  return r.json().catch(() => ({}))
}

export const api = {
  config: () => getJSON<Config>('/api/config'),
  sessions: () => getJSON<{ sessions: SessionMeta[] }>('/api/sessions').then((d) => d.sessions),
  messages: () => getJSON<{ messages: ServerMessage[] }>('/api/messages').then((d) => d.messages),
  personas: () =>
    getJSON<{ personas: Persona[]; active: string }>('/api/personas'),
  newSession: () => postJSON('/api/session/new'),
  resumeSession: (id: string) => postJSON('/api/session/resume', { id }),
  reload: () => postJSON('/api/session/reload'),
  login: async (password: string): Promise<boolean> => {
    const r = await fetch('/api/login', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ password }),
    })
    return r.ok
  },
  logout: () => postJSON('/api/logout'),
  // Probe a protected route to see if the current cookie is valid.
  checkAuth: async (): Promise<boolean> => (await fetch('/api/sessions')).ok,
  // Control center.
  cron: () => getJSON<{ jobs: CronJob[] }>('/api/cron').then((d) => d.jobs),
  cronAdd: (name: string, schedule: string, prompt: string) =>
    postJSON('/api/cron', { name, schedule, prompt }),
  cronRemove: (id: string) => postJSON('/api/cron/remove', { id }),
  skillsList: () => getJSON<{ skills: SkillFull[] }>('/api/skills').then((d) => d.skills),
  memoryGet: () => getJSON<{ memory: string; user: string }>('/api/memory'),
  memorySet: (which: 'memory' | 'user', content: string) =>
    postJSON('/api/memory', { which, content }),
  usage: () => getJSON<{ usage: Usage }>('/api/usage').then((d) => d.usage),
  settingsGet: () =>
    getJSON<{ settings: SettingsView }>('/api/settings').then((d) => d.settings),
  settingsSet: (patch: Record<string, unknown>) => postJSON('/api/settings', patch),
  // Voice.
  transcribe: async (blob: Blob): Promise<string> => {
    const r = await fetch('/api/voice/transcribe', {
      method: 'POST',
      headers: { 'content-type': blob.type || 'audio/webm' },
      body: blob,
    })
    if (!r.ok) return ''
    const d = await r.json().catch(() => ({}) as { text?: string })
    return d.text || ''
  },
  speak: async (text: string): Promise<void> => {
    const r = await fetch('/api/voice/speak', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ text }),
    })
    if (!r.ok) return
    const url = URL.createObjectURL(await r.blob())
    const audio = new Audio(url)
    audio.onended = () => URL.revokeObjectURL(url)
    await audio.play().catch(() => {})
  },
  send: (text: string) => postJSON('/api/chat/send', { text }),
  cancel: () => postJSON('/api/chat/cancel'),
  compact: () => postJSON('/api/compact'),
  undo: () => postJSON('/api/undo'),
  models: () => getJSON<{ options: ModelOptions }>('/api/models').then((d) => d.options),
  setModel: (model: string) => postJSON('/api/model/set', { model }),
  setProvider: (provider: string, api_key?: string) =>
    postJSON('/api/provider/set', { provider, api_key }),
  setPersona: (name: string) => postJSON('/api/persona/set', { name }),
  setYolo: (on: boolean) => postJSON('/api/yolo', { on }),
  approve: (request_id: string, decision: 'allow' | 'deny', scope: 'once' | 'session') =>
    postJSON('/api/approval/respond', { request_id, decision, scope }),
  clarify: (request_id: string, value: string) =>
    postJSON('/api/clarify/respond', { request_id, value }),
}

/** All SSE event names the core emits (axum sets `event:` to these). */
export const SSE_EVENTS = [
  'turn_started',
  'assistant_started',
  'token',
  'thinking',
  'assistant_finished',
  'tool_start',
  'tool_progress',
  'tool_result',
  'diff',
  'approval_request',
  'clarify_request',
  'todo_update',
  'usage',
  'compaction',
  'done',
  'notice',
  'reload',
  'error',
] as const
