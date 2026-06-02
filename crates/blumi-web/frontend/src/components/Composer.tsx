import { useState } from 'react'

type Props = {
  busy: boolean
  onSend: (text: string) => void
  onCancel: () => void
}

export function Composer({ busy, onSend, onCancel }: Props) {
  const [text, setText] = useState('')

  function submit() {
    const t = text.trim()
    if (!t || busy) return
    onSend(t)
    setText('')
  }

  return (
    <div className="composer">
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault()
            submit()
          }
        }}
        placeholder="Ask blumi to build, fix, or explain…  (Enter to send · Shift+Enter for newline)"
        rows={2}
      />
      {busy ? (
        <button className="btn deny" onClick={onCancel}>
          Stop
        </button>
      ) : (
        <button className="btn send" onClick={submit} disabled={!text.trim()}>
          Send
        </button>
      )}
    </div>
  )
}
