export type ToolEntry = {
  kind: 'tool'
  id: string
  name: string
  summary: string
  ok: boolean | null
  preview?: string
  diff?: string
  diffStat?: string
}

export type Entry =
  | { kind: 'user'; text: string }
  | { kind: 'assistant'; text: string }
  | { kind: 'notice'; text: string; error?: boolean }
  | ToolEntry

export type TodoStatus = 'pending' | 'in_progress' | 'completed'
export type Todo = { id: string; content: string; status: TodoStatus }

export type Approval = {
  request_id: string
  tool: string
  summary: string
  dangerous: boolean
  diff?: string | null
}

export type ClarifyChoice = { id: string; label: string }
export type Clarify = {
  request_id: string
  question: string
  choices: ClarifyChoice[]
}

export type SessionMeta = {
  id: string
  title: string
  model: string
  message_count: number
}

/** A message from the server snapshot, used to restore a transcript. */
export type ServerMessage = {
  role: 'user' | 'assistant' | 'tool'
  text: string
  tool_name?: string | null
}

export type Persona = { name: string; description: string }

export type CronJob = { id: string; name: string; schedule: string; prompt: string }
export type SkillFull = { name: string; description: string; body: string }
export type ModelUsage = {
  model: string
  sessions: number
  input_tokens: number
  output_tokens: number
}
export type Usage = {
  sessions: number
  messages: number
  input_tokens: number
  output_tokens: number
  by_model: ModelUsage[]
}

export type Config = {
  model: string
  models: string[]
  working_dir: string
  version: string
  persona: string
  context_size: number
  auth_required: boolean
  voice_enabled: boolean
}
