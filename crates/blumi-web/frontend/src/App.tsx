import { useEffect, useReducer, useRef, useState } from 'react'
import { api, SSE_EVENTS } from './api'
import type { Approval, Clarify, Config, Entry, SessionMeta, Todo } from './types'
import { Header } from './components/Header'
import { Sidebar } from './components/Sidebar'
import { RunPanel } from './components/RunPanel'
import { Message } from './components/Message'
import { Composer } from './components/Composer'
import { ApprovalCard } from './components/ApprovalCard'
import { ClarifyCard } from './components/ClarifyCard'

type State = {
  entries: Entry[]
  streaming: string | null
  thinking: string | null
  busy: boolean
  approval: Approval | null
  clarify: Clarify | null
  todos: Todo[]
  usage: { input: number; output: number }
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
}

type Action =
  | { type: 'sse'; name: string; data: any }
  | { type: 'user'; text: string }
  | { type: 'clearApproval' }
  | { type: 'clearClarify' }
  | { type: 'reset' }

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

function reducer(s: State, a: Action): State {
  switch (a.type) {
    case 'user':
      return { ...s, entries: [...s.entries, { kind: 'user', text: a.text }], busy: true }
    case 'clearApproval':
      return { ...s, approval: null }
    case 'clearClarify':
      return { ...s, clarify: null }
    case 'reset':
      return initial
    case 'sse':
      return applyEvent(s, a.name, a.data)
  }
}

export function App() {
  const [state, dispatch] = useReducer(reducer, initial)
  const [config, setConfig] = useState<Config | null>(null)
  const [sessions, setSessions] = useState<SessionMeta[]>([])
  const [connected, setConnected] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    api.config().then(setConfig).catch(() => {})
    api.sessions().then(setSessions).catch(() => {})
  }, [])

  useEffect(() => {
    const es = new EventSource('/api/chat/stream')
    es.onopen = () => setConnected(true)
    es.onerror = () => setConnected(false)
    const handler = (e: MessageEvent) => {
      // The browser fires a native, data-less `error` event on connection
      // issues/reconnects — ignore those (handled by onerror), only process
      // real server events, which always carry a JSON `data` payload.
      if (!e.data) return
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
  }, [])

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

  const empty = state.entries.length === 0 && !state.streaming

  return (
    <div className="app">
      <Header config={config} connected={connected} />
      <div className="main">
        <Sidebar sessions={sessions} onNew={() => dispatch({ type: 'reset' })} />
        <section className="chat">
          <div className="transcript" ref={scrollRef}>
            {empty && <Landing />}
            {state.entries.map((e, i) => (
              <Message entry={e} key={i} />
            ))}
            {state.thinking && <div className="thinking">✿ thinking…</div>}
            {state.streaming && <Message entry={{ kind: 'assistant', text: state.streaming }} />}
            {state.clarify && <ClarifyCard clarify={state.clarify} onRespond={respondClarify} />}
            {state.approval && <ApprovalCard approval={state.approval} onRespond={respondApproval} />}
          </div>
          <Composer busy={state.busy} onSend={send} onCancel={() => api.cancel()} />
        </section>
        <RunPanel todos={state.todos} usage={state.usage} model={config?.model ?? ''} busy={state.busy} />
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
