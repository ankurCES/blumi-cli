import { useEffect, useReducer, useRef, useState } from 'react'
import { api, SSE_EVENTS } from './api'
import type { Approval, Clarify, Config, Entry, Persona, ServerMessage, SessionMeta, Todo } from './types'
import { Header } from './components/Header'
import { Sidebar } from './components/Sidebar'
import { RunPanel } from './components/RunPanel'
import { Message } from './components/Message'
import { Composer } from './components/Composer'
import { ApprovalCard } from './components/ApprovalCard'
import { ClarifyCard } from './components/ClarifyCard'
import { Thinking } from './components/Thinking'

type State = {
  entries: Entry[]
  streaming: string | null
  thinking: string | null
  busy: boolean
  approval: Approval | null
  clarify: Clarify | null
  todos: Todo[]
  usage: { input: number; output: number }
  contextTokens: number
}

const initial: State = {
  entries: [],
  streaming: null,
  thinking: null,
  busy: false,
  approval: null,
  clarify: null,
  todos: [],
  usage: { input: 0, output: 0 },
  contextTokens: 0,
}

type Action =
  | { type: 'sse'; name: string; data: any }
  | { type: 'user'; text: string }
  | { type: 'load'; messages: ServerMessage[] }
  | { type: 'clearApproval' }
  | { type: 'clearClarify' }

function firstLine(s: string): string {
  return (s ?? '').split('\n')[0] ?? ''
}

function applyEvent(s: State, name: string, d: any): State {
  switch (name) {
    case 'turn_started':
      return { ...s, busy: true }
    case 'assistant_started':
      return { ...s, streaming: '' }
    case 'token':
      return { ...s, streaming: (s.streaming ?? '') + (d.text ?? '') }
    case 'thinking':
      return { ...s, thinking: (s.thinking ?? '') + (d.text ?? '') }
    case 'assistant_finished': {
      const text = (s.streaming ?? '').trim()
      if (text) {
        return { ...s, entries: [...s.entries, { kind: 'assistant', text: s.streaming! }], streaming: null }
      }
      return { ...s, streaming: null }
    }
    case 'tool_start':
      return {
        ...s,
        entries: [
          ...s.entries,
          { kind: 'tool', id: d.id, name: d.name, summary: d.summary ?? '', ok: null },
        ],
      }
    case 'tool_result':
      return {
        ...s,
        entries: s.entries.map((e) =>
          e.kind === 'tool' && e.id === d.id ? { ...e, ok: d.ok, preview: firstLine(d.preview) } : e,
        ),
      }
    case 'diff':
      return {
        ...s,
        entries: s.entries.map((e) =>
          e.kind === 'tool' && e.id === d.id
            ? { ...e, diff: d.unified, diffStat: `+${d.additions} -${d.deletions}` }
            : e,
        ),
      }
    case 'approval_request':
      return { ...s, approval: d as Approval }
    case 'clarify_request':
      return { ...s, clarify: d as Clarify }
    case 'todo_update':
      return { ...s, todos: (d.items ?? []) as Todo[] }
    case 'usage':
      return {
        ...s,
        usage: { input: s.usage.input + (d.input ?? 0), output: s.usage.output + (d.output ?? 0) },
        // The latest request's input ≈ current context usage.
        contextTokens: d.input ?? s.contextTokens,
      }
    case 'compaction':
      return {
        ...s,
        entries: [
          ...s.entries,
          { kind: 'notice', text: `context compacted (${d.messages_compressed} messages)` },
        ],
      }
    case 'notice':
      return { ...s, entries: [...s.entries, { kind: 'notice', text: d.message ?? '' }] }
    case 'error':
      return { ...s, entries: [...s.entries, { kind: 'notice', text: d.message ?? 'error', error: true }] }
    case 'done': {
      const leftover = (s.streaming ?? '').trim()
      const entries = leftover ? [...s.entries, { kind: 'assistant' as const, text: s.streaming! }] : s.entries
      return { ...s, entries, streaming: null, thinking: null, busy: false }
    }
    default:
      return s
  }
}

function messagesToEntries(messages: ServerMessage[]): Entry[] {
  return messages.map((m): Entry => {
    if (m.role === 'user') return { kind: 'user', text: m.text }
    if (m.role === 'assistant') return { kind: 'assistant', text: m.text }
    return {
      kind: 'tool',
      id: `r${Math.random().toString(36).slice(2)}`,
      name: m.tool_name || 'tool',
      summary: '',
      ok: true,
      preview: firstLine(m.text),
    }
  })
}

function reducer(s: State, a: Action): State {
  switch (a.type) {
    case 'user':
      return { ...s, entries: [...s.entries, { kind: 'user', text: a.text }], busy: true }
    case 'load':
      return { ...initial, entries: messagesToEntries(a.messages) }
    case 'clearApproval':
      return { ...s, approval: null }
    case 'clearClarify':
      return { ...s, clarify: null }
    case 'sse':
      return applyEvent(s, a.name, a.data)
  }
}

export function App() {
  const [state, dispatch] = useReducer(reducer, initial)
  const [config, setConfig] = useState<Config | null>(null)
  const [sessions, setSessions] = useState<SessionMeta[]>([])
  const [personas, setPersonas] = useState<Persona[]>([])
  const [persona, setPersona] = useState('default')
  const [yolo, setYolo] = useState(false)
  const [connected, setConnected] = useState(false)
  // Bumped on a session switch to re-subscribe SSE + reload the transcript.
  const [epoch, setEpoch] = useState(0)
  const [start, setStart] = useState(() => Date.now())
  const [activeSecs, setActiveSecs] = useState(0)
  const [nowTs, setNowTs] = useState(() => Date.now())
  const scrollRef = useRef<HTMLDivElement>(null)

  const refreshSessions = () => api.sessions().then(setSessions).catch(() => {})

  // Static config + lists (once).
  useEffect(() => {
    api
      .config()
      .then((c) => {
        setConfig(c)
        if (c.persona) setPersona(c.persona)
      })
      .catch(() => {})
    refreshSessions()
    api
      .personas()
      .then((p) => {
        setPersonas(p.personas)
        if (p.active) setPersona(p.active)
      })
      .catch(() => {})
  }, [])

  // Restore the current session's transcript on load + after a switch.
  useEffect(() => {
    api.messages().then((ms) => dispatch({ type: 'load', messages: ms })).catch(() => {})
  }, [epoch])

  // SSE — re-subscribed on each session switch.
  useEffect(() => {
    const es = new EventSource('/api/chat/stream')
    es.onopen = () => setConnected(true)
    es.onerror = () => setConnected(false)
    const handler = (e: MessageEvent) => {
      if (!e.data) return // native data-less error/keep-alive
      let data: any = {}
      try {
        data = JSON.parse(e.data)
      } catch {
        return
      }
      dispatch({ type: 'sse', name: e.type, data })
    }
    for (const name of SSE_EVENTS) es.addEventListener(name, handler as EventListener)
    return () => es.close()
  }, [epoch])

  // Uptime clock + active-with-bot accumulation.
  useEffect(() => {
    const t = setInterval(() => setNowTs(Date.now()), 1000)
    return () => clearInterval(t)
  }, [])
  useEffect(() => {
    if (!state.busy) return
    const t = setInterval(() => setActiveSecs((n) => n + 1), 1000)
    return () => clearInterval(t)
  }, [state.busy])

  useEffect(() => {
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [state.entries, state.streaming, state.thinking])

  function send(text: string) {
    dispatch({ type: 'user', text })
    api.send(text)
  }
  function respondApproval(decision: 'allow' | 'deny', scope: 'once' | 'session') {
    if (!state.approval) return
    api.approve(state.approval.request_id, decision, scope)
    dispatch({ type: 'clearApproval' })
  }
  function respondClarify(value: string) {
    if (!state.clarify) return
    api.clarify(state.clarify.request_id, value)
    dispatch({ type: 'clearClarify' })
  }
  function changePersona(name: string) {
    setPersona(name)
    api.setPersona(name)
  }
  function toggleYolo(on: boolean) {
    setYolo(on)
    api.setYolo(on)
  }
  async function newSession() {
    await api.newSession()
    setStart(Date.now())
    setActiveSecs(0)
    setEpoch((e) => e + 1)
    refreshSessions()
  }
  async function resumeSession(id: string) {
    await api.resumeSession(id)
    setStart(Date.now())
    setActiveSecs(0)
    setEpoch((e) => e + 1)
    refreshSessions()
  }

  const empty = state.entries.length === 0 && !state.streaming
  const showThinking = state.busy && !state.streaming
  const uptimeSecs = Math.max(0, Math.floor((nowTs - start) / 1000))

  return (
    <div className="app">
      <Header
        config={config}
        connected={connected}
        personas={personas}
        persona={persona}
        onPersona={changePersona}
        yolo={yolo}
        onYolo={toggleYolo}
        busy={state.busy}
        onCompact={() => api.compact()}
        onUndo={() => api.undo()}
      />
      <div className="main">
        <Sidebar sessions={sessions} onNew={newSession} onResume={resumeSession} />
        <section className="chat">
          <div className="transcript" ref={scrollRef}>
            {empty && <Landing />}
            {state.entries.map((e, i) => (
              <Message entry={e} key={i} />
            ))}
            {state.streaming && <Message entry={{ kind: 'assistant', text: state.streaming }} />}
            {showThinking && <Thinking text={state.thinking ?? undefined} />}
            {state.clarify && <ClarifyCard clarify={state.clarify} onRespond={respondClarify} />}
            {state.approval && <ApprovalCard approval={state.approval} onRespond={respondApproval} />}
          </div>
          <Composer busy={state.busy} onSend={send} onCancel={() => api.cancel()} />
        </section>
        <RunPanel
          todos={state.todos}
          usage={state.usage}
          model={config?.model ?? ''}
          persona={persona}
          busy={state.busy}
          contextTokens={state.contextTokens}
          contextSize={config?.context_size ?? 0}
          uptimeSecs={uptimeSecs}
          activeSecs={activeSecs}
        />
      </div>
    </div>
  )
}

function Landing() {
  return (
    <div className="landing">
      <div className="landing-flower">✿</div>
      <div className="landing-word">blumi</div>
      <div className="landing-tag">the local-first agentic coding companion</div>
      <div className="landing-hint">Type a message below to start.</div>
    </div>
  )
}
