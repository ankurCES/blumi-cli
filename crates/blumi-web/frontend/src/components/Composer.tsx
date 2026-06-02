import { useRef, useState } from 'react'
import { api } from '../api'

type Props = {
  busy: boolean
  onSend: (text: string) => void
  onCancel: () => void
  voiceEnabled: boolean
}

export function Composer({ busy, onSend, onCancel, voiceEnabled }: Props) {
  const [text, setText] = useState('')
  const [recording, setRecording] = useState(false)
  const [transcribing, setTranscribing] = useState(false)
  const recRef = useRef<MediaRecorder | null>(null)
  const chunksRef = useRef<Blob[]>([])

  function submit() {
    const t = text.trim()
    if (!t || busy) return
    onSend(t)
    setText('')
  }

  async function toggleMic() {
    if (recording) {
      recRef.current?.stop()
      return
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      const rec = new MediaRecorder(stream)
      chunksRef.current = []
      rec.ondataavailable = (e) => {
        if (e.data.size) chunksRef.current.push(e.data)
      }
      rec.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop())
        setRecording(false)
        const blob = new Blob(chunksRef.current, { type: rec.mimeType || 'audio/webm' })
        setTranscribing(true)
        const t = await api.transcribe(blob).catch(() => '')
        setTranscribing(false)
        if (t) setText((cur) => (cur ? cur + ' ' : '') + t)
      }
      rec.start()
      recRef.current = rec
      setRecording(true)
    } catch {
      /* mic permission denied / unavailable */
    }
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
        placeholder={
          transcribing
            ? 'transcribing…'
            : 'Ask blumi to build, fix, or explain…  (Enter to send · Shift+Enter for newline)'
        }
        rows={2}
      />
      {voiceEnabled && (
        <button
          className={`btn mic ${recording ? 'rec' : ''}`}
          onClick={toggleMic}
          title={recording ? 'Stop recording' : 'Record a voice message'}
        >
          {recording ? '⏺' : '🎤'}
        </button>
      )}
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
