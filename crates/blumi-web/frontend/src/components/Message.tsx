import type { Entry } from '../types'
import { Md } from './Md'
import { ToolCard } from './ToolCard'

export function Message({ entry }: { entry: Entry }) {
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
