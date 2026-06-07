// blumi service worker — offline app shell, network-first.
// The API and SSE stream are NEVER cached (always go to the network).
const CACHE = 'blumi-v1'

self.addEventListener('install', () => self.skipWaiting())

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  )
})

// Web Push (#209d): show a notification for an incoming push. Payload is the
// JSON the server sent ({ title, body }); fall back gracefully if absent.
self.addEventListener('push', (e) => {
  let data = { title: 'blumi', body: 'Run complete' }
  try {
    if (e.data) data = { ...data, ...e.data.json() }
  } catch {
    /* non-JSON payload — keep defaults */
  }
  e.waitUntil(
    self.registration.showNotification(data.title || 'blumi', {
      body: data.body || '',
      icon: '/icon.svg',
      badge: '/icon.svg',
      tag: 'blumi-completion',
    }),
  )
})

// Focus (or open) the app when a notification is clicked.
self.addEventListener('notificationclick', (e) => {
  e.notification.close()
  e.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((cs) => {
      for (const c of cs) {
        if ('focus' in c) return c.focus()
      }
      return self.clients.openWindow ? self.clients.openWindow('/') : undefined
    }),
  )
})

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url)
  // Bypass anything that isn't a same-origin GET, and all API/SSE traffic.
  if (e.request.method !== 'GET' || url.origin !== location.origin || url.pathname.startsWith('/api')) {
    return
  }
  e.respondWith(
    fetch(e.request)
      .then((resp) => {
        const copy = resp.clone()
        caches.open(CACHE).then((c) => c.put(e.request, copy))
        return resp
      })
      .catch(() => caches.match(e.request).then((r) => r || caches.match('/'))),
  )
})
