import type { Approval } from '../types'

type Props = {
  approval: Approval
  onRespond: (decision: 'allow' | 'deny', scope: 'once' | 'session') => void
}

export function ApprovalCard({ approval, onRespond }: Props) {
  return (
    <div className={`approval ${approval.dangerous ? 'danger' : ''}`}>
      <div className="approval-head">
        <span className="approval-title">
          {approval.dangerous ? '⚠ approve tool' : 'approve tool'}
        </span>
        <span className="approval-tool">{approval.tool}</span>
      </div>
      <div className="approval-summary">{approval.summary}</div>
      {approval.advice && <div className="approval-advice">{approval.advice}</div>}
      {approval.diff && <pre className="diff small">{approval.diff}</pre>}
      <div className="approval-actions">
        <button className="btn allow" onClick={() => onRespond('allow', 'once')}>
          Allow once
        </button>
        <button className="btn allow-session" onClick={() => onRespond('allow', 'session')}>
          Allow for session
        </button>
        <button className="btn deny" onClick={() => onRespond('deny', 'once')}>
          Deny
        </button>
      </div>
    </div>
  )
}
