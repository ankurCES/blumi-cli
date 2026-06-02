import { useState } from 'react'
import type { Clarify } from '../types'

type Props = {
  clarify: Clarify
  onRespond: (value: string) => void
}

export function ClarifyCard({ clarify, onRespond }: Props) {
  const [value, setValue] = useState('')
  return (
    <div className="clarify">
      <div className="clarify-q">{clarify.question}</div>
      {clarify.choices.length > 0 ? (
        <div className="clarify-choices">
          {clarify.choices.map((c) => (
            <button className="btn" key={c.id} onClick={() => onRespond(c.id)}>
              {c.label}
            </button>
          ))}
        </div>
      ) : (
        <form
          className="clarify-form"
          onSubmit={(e) => {
            e.preventDefault()
            onRespond(value)
          }}
        >
          <input
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder="your answer…"
            autoFocus
          />
          <button className="btn allow" type="submit">
            Send
          </button>
        </form>
      )}
    </div>
  )
}
