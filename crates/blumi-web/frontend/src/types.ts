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
  /** Local-LLM "brain" recommendation (advisory mode / auto-mode escalation). */
  advice?: string | null
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

export type ProviderOption = { name: string; label: string; ready: boolean }
export type ModelOptions = {
  provider: string
  model: string
  models: string[]
  providers: ProviderOption[]
}

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

export type SettingsView = {
  brain: {
    mode: string
    provider: string
    model: string
  }
  voice: {
    enabled: boolean
    stt_base_url: string
    stt_model: string
    tts_provider: string
    tts_base_url: string
    tts_model: string
    tts_voice: string
    api_key_set: boolean
    tts_api_key_set: boolean
  }
  gateway: {
    yolo: boolean
    telegram_token_set: boolean
    discord_token_set: boolean
    slack_bot_token_set: boolean
    slack_app_token_set: boolean
    whatsapp_token_set: boolean
    whatsapp_phone_number_id: string
    whatsapp_verify_token: string
  }
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

export interface RouteTier {
  model: string
  turns: number
  input: number
  output: number
  cost_usd: number
}
export interface RouteStatus {
  mode: string
  light?: RouteTier
  heavy?: RouteTier
  judge?: RouteTier
  actual_cost_usd?: number
  all_heavy_cost_usd?: number
  saved_usd?: number
}

export interface MemoryEntry {
  id: number
  namespace: string
  kind: string
  text: string
  origin: string
  created_at: string
  updated_at: string
  hits: number
  utility: number
  status: string
  pinned: boolean
}

export interface AlwaysOnStatus {
  enabled: boolean
  autonomy?: string
  recent: string[]
  reports: string[]
}

export interface GitView {
  ok: boolean
  text: string
}
