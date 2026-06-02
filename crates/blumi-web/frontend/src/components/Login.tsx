import { useState } from 'react'
import { api } from '../api'

type Props = { onAuth: () => void }

export function Login({ onAuth }: Props) {
  const [password, setPassword] = useState('')
  const [error, setError] = useState(false)
  const [busy, setBusy] = useState(false)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(false)
    const ok = await api.login(password)
    setBusy(false)
    if (ok) onAuth()
    else {
      setError(true)
      setPassword('')
    }
  }

  return (
    <div className="login-screen">
      <form className="login-card" onSubmit={submit}>
        <div className="login-flower">✿</div>
        <div className="login-word">blumi</div>
        <div className="login-tag">Enter your password to continue</div>
        <input
          className="login-input"
          type="password"
          value={password}
          autoFocus
          placeholder="password"
          onChange={(e) => setPassword(e.target.value)}
        />
        {error && <div className="login-error">Incorrect password</div>}
        <button className="login-btn" type="submit" disabled={busy || !password}>
          {busy ? 'Signing in…' : 'Sign in'}
        </button>
      </form>
    </div>
  )
}
