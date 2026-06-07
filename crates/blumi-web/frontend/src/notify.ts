import { api } from './api'

// Browser in-tab completion alert (#209b). When a turn finishes while this tab is
// backgrounded, nudge the user: flash the title, badge the favicon, play a short
// ping, and drop an in-page toast. Self-contained, dependency-free, best-effort —
// each effect guards its own browser API, and nothing fires while the tab is
// focused (so it never interrupts active use).
//
// Always-on / `blumi loop` completions run off-session (a different process), so
// they reach you via the desktop / bot / web-push channels instead — this file is
// only about the interactive turn you started in this tab.

let armed = false
let flashTimer: number | null = null
let originalTitle = ''
let originalFavicon: string | null = null

/// Arm on turn start. A `done` only fires the alert if a turn was armed, so a
/// stale `done` replayed from the SSE backlog on reconnect can't alert spuriously.
export function armCompletionAlert(): void {
  armed = true
}

/// Fire iff a turn was armed AND the tab is hidden; disarms on every completion.
export function fireCompletionAlertIfHidden(text: string): void {
  const wasArmed = armed
  armed = false
  if (!wasArmed) return
  if (typeof document === 'undefined' || !document.hidden) return
  flashTitle()
  badgeFavicon()
  ping()
  toast(text)
}

function clearAlert(): void {
  if (flashTimer !== null) {
    clearInterval(flashTimer)
    flashTimer = null
  }
  if (originalTitle) document.title = originalTitle
  restoreFavicon()
  window.removeEventListener('focus', clearAlert)
  document.removeEventListener('visibilitychange', onVisible)
}

function onVisible(): void {
  if (!document.hidden) clearAlert()
}

function flashTitle(): void {
  if (flashTimer !== null) return // already flashing
  originalTitle = document.title
  let on = false
  flashTimer = window.setInterval(() => {
    on = !on
    document.title = on ? '✿ blumi — done!' : originalTitle
  }, 1000)
  window.addEventListener('focus', clearAlert)
  document.addEventListener('visibilitychange', onVisible)
}

function badgeFavicon(): void {
  try {
    const link = document.querySelector<HTMLLinkElement>('link[rel="icon"]')
    if (!link) return
    if (originalFavicon === null) originalFavicon = link.getAttribute('href')
    const canvas = document.createElement('canvas')
    canvas.width = 32
    canvas.height = 32
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    ctx.fillStyle = '#16161e'
    ctx.fillRect(0, 0, 32, 32)
    ctx.beginPath()
    ctx.arc(22, 10, 9, 0, Math.PI * 2)
    ctx.fillStyle = '#ff5fa2'
    ctx.fill()
    link.setAttribute('href', canvas.toDataURL('image/png'))
  } catch {
    /* favicon badge is optional */
  }
}

function restoreFavicon(): void {
  if (originalFavicon === null) return
  const link = document.querySelector<HTMLLinkElement>('link[rel="icon"]')
  if (link) link.setAttribute('href', originalFavicon)
  originalFavicon = null
}

function ping(): void {
  try {
    const Ctx: typeof AudioContext | undefined =
      window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
    if (!Ctx) return
    const ac = new Ctx()
    const play = () => {
      const o = ac.createOscillator()
      const g = ac.createGain()
      o.connect(g)
      g.connect(ac.destination)
      o.type = 'sine'
      o.frequency.value = 880
      g.gain.setValueAtTime(0.0001, ac.currentTime)
      g.gain.exponentialRampToValueAtTime(0.2, ac.currentTime + 0.02)
      g.gain.exponentialRampToValueAtTime(0.0001, ac.currentTime + 0.4)
      o.start()
      o.stop(ac.currentTime + 0.42)
      o.onended = () => ac.close().catch(() => {})
    }
    // Autoplay policy may leave the context suspended until a gesture; the user
    // started this turn, so a resume usually succeeds.
    const resumed = ac.resume?.()
    if (resumed && typeof resumed.then === 'function') resumed.then(play).catch(() => {})
    else play()
  } catch {
    /* audio is optional */
  }
}

// --- Web Push (#209d) ---

/// Web Push needs a secure context (HTTPS or `http://localhost`) plus the SW +
/// Push + Notification APIs. On a plain-HTTP LAN page this is false, so the UI
/// can explain why the button is dormant.
export function isPushSupported(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.isSecureContext &&
    'serviceWorker' in navigator &&
    'PushManager' in window &&
    'Notification' in window
  )
}

function urlBase64ToUint8Array(base64: string): Uint8Array<ArrayBuffer> {
  const padding = '='.repeat((4 - (base64.length % 4)) % 4)
  const b64 = (base64 + padding).replace(/-/g, '+').replace(/_/g, '/')
  const raw = atob(b64)
  // Back the array with an explicit ArrayBuffer so the type satisfies
  // BufferSource (TS 5.7 made Uint8Array generic over ArrayBufferLike).
  const out = new Uint8Array(new ArrayBuffer(raw.length))
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i)
  return out
}

/// Subscribe this browser to Web Push: request permission, fetch the VAPID key,
/// subscribe via the service worker, and register the subscription server-side.
/// Returns a human-readable status; no-ops cleanly when unsupported.
export async function enableWebPush(): Promise<string> {
  if (!isPushSupported()) {
    return 'Web push needs a secure context (HTTPS or localhost).'
  }
  try {
    const perm = await Notification.requestPermission()
    if (perm !== 'granted') return 'Notification permission denied.'
    const key = await api.pushKey()
    if (!key) return 'Server has no VAPID key.'
    const reg = await navigator.serviceWorker.ready
    const existing = await reg.pushManager.getSubscription()
    const sub =
      existing ??
      (await reg.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: urlBase64ToUint8Array(key),
      }))
    const res = await api.pushSubscribe(sub.toJSON())
    return res.ok ? 'Web push enabled on this browser.' : `Failed: ${res.error ?? 'unknown'}`
  } catch (e) {
    return `Web push failed: ${e instanceof Error ? e.message : String(e)}`
  }
}

function toast(text: string): void {
  try {
    const el = document.createElement('div')
    el.className = 'blumi-toast'
    el.textContent = `✿ ${text}`
    el.onclick = () => {
      window.focus()
      el.remove()
    }
    document.body.appendChild(el)
    setTimeout(() => el.remove(), 8000)
  } catch {
    /* toast is optional */
  }
}
