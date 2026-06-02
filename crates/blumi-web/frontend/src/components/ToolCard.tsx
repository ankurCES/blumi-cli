import type { ToolEntry } from '../types'

export function ToolCard({ tool }: { tool: ToolEntry }) {
  const status =
    tool.ok === null ? 'running' : tool.ok ? 'ok' : 'failed'
  const glyph = tool.ok === null ? '●' : tool.ok ? '✓' : '×'
  return (
    <div className={`tool ${status}`}>
      <div className="tool-head">
        <span className="tool-glyph">{glyph}</span>
        <span className="tool-name">{tool.name}</span>
        {tool.diffStat && <span className="tool-stat">{tool.diffStat}</span>}
        <span className="tool-summary">{tool.summary}</span>
      </div>
      {tool.preview && <div className="tool-preview">{tool.preview}</div>}
      {tool.diff && <Diff unified={tool.diff} />}
    </div>
  )
}

function Diff({ unified }: { unified: string }) {
  return (
    <pre className="diff">
      {unified.split('\n').map((line, i) => {
        let cls = 'ctx'
        if (line.startsWith('+') && !line.startsWith('+++')) cls = 'add'
        else if (line.startsWith('-') && !line.startsWith('---')) cls = 'del'
        else if (line.startsWith('@@')) cls = 'hunk'
        return (
          <div className={`dl ${cls}`} key={i}>
            {line || ' '}
          </div>
        )
      })}
    </pre>
  )
}
