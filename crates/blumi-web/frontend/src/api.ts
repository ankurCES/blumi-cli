import type { Config, Persona, ServerMessage, SessionMeta } from './types'

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
  send: (text: string) => postJSON('/api/chat/send', { text }),
  cancel: () => postJSON('/api/chat/cancel'),
  compact: () => postJSON('/api/compact'),
  undo: () => postJSON('/api/undo'),
  setModel: (model: string) => postJSON('/api/model/set', { model }),
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
  'error',
] as const
