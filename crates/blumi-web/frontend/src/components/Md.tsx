import type { ReactNode } from 'react'

// A deliberately small markdown renderer: fenced code blocks + inline code, with
// whitespace preserved for prose. Keeps the bundle dependency-free; the heavy
// markdown/syntax work already lives in the TUI renderer.
export function Md({ text }: { text: string }) {
  const parts = text.split('```')
  return (
    <div className="md">
      {parts.map((part, i) =>
        i % 2 === 1 ? (
          <pre className="code" key={i}>
            <code>{stripLang(part)}</code>
          </pre>
        ) : (
          <p className="prose" key={i}>
            {inline(part)}
          </p>
        ),
      )}
    </div>
  )
}

function stripLang(block: string): string {
  // Drop a leading ```lang line's language token.
  const nl = block.indexOf('\n')
  if (nl === -1) return block
  const first = block.slice(0, nl).trim()
  if (/^[a-zA-Z0-9+#.-]*$/.test(first)) return block.slice(nl + 1)
  return block
}

function inline(text: string): ReactNode[] {
  // Split on single backticks → inline code spans.
  return text.split('`').map((seg, i) =>
    i % 2 === 1 ? (
      <code className="inline" key={i}>
        {seg}
      </code>
    ) : (
      <span key={i}>{seg}</span>
    ),
  )
}
