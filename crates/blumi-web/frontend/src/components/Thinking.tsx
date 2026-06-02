// The animated thinking mascot — a colour-sweeping, blooming flower with
// staggered dots, the web echo of the TUI's animated rose.
export function Thinking({ text }: { text?: string }) {
  return (
    <div className="thinking-mascot" aria-live="polite">
      <span className="tflower">✿</span>
      <span className="tlabel">{text && text.trim() ? text : 'thinking'}</span>
      <span className="tdots">
        <i />
        <i />
        <i />
      </span>
    </div>
  )
}
