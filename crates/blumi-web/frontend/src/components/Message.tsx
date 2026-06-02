import type { Entry } from '../types'
import { api } from '../api'
import { Md } from './Md'
import { ToolCard } from './ToolCard'

export function Message({ entry, voiceEnabled }: { entry: Entry; voiceEnabled?: boolean }) {
  switch (entry.kind) {
    case 'user':
      return (
        <div className="msg user">
          <div className="avatar">you</div>
          <div className="bubble">{entry.text}</div>
        </div>
      )
    case 'assistant':
      return (
        <div className="msg assistant">
          <div className="avatar">✿</div>
          <div className="bubble">
            <Md text={entry.text} />
            {voiceEnabled && entry.text.trim() && (
              <button
                className="speak-btn"
                title="Read aloud"
                onClick={() => api.speak(entry.text)}
              >
                🔊
              </button>
            )}
          </div>
        </div>
      )
    case 'tool':
      return (
        <div className="msg tool-row">
          <ToolCard tool={entry} />
        </div>
      )
    case 'notice':
      return (
        <div className={`notice ${entry.error ? 'err' : ''}`}>{entry.text}</div>
      )
  }
}
