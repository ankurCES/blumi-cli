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
