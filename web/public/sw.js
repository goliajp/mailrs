// CACHE_NAME bump (v1 -> v2) on 2026-06-04 is intentional: it forces every
// existing client to drop the old mailrs-v1 cache during activate, which
// was holding a stale index.html that referenced JS bundle hashes the new
// server doesn't serve (caused white-screen + "text/html is not a valid
// JavaScript MIME type" after the v1.7.99 deploy).
const CACHE_NAME = 'mailrs-v2'
const SHELL_ASSETS = ['/icon.svg', '/icon-192.png', '/icon-512.png', '/offline.html']

// install: cache static shell assets ONLY (no index.html — see fetch handler)
self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE_NAME).then((cache) => cache.addAll(SHELL_ASSETS)))
  self.skipWaiting()
})

// activate: clean old caches, take over from any existing SW immediately
self.addEventListener('activate', (event) => {
  event.waitUntil(
    Promise.all([
      caches.keys().then((keys) =>
        Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k))),
      ),
      self.clients.claim(),
    ]),
  )
})

self.addEventListener('fetch', (event) => {
  const req = event.request
  if (req.method !== 'GET') return

  const url = new URL(req.url)

  // navigation requests (top-level HTML): ALWAYS network-first.
  // Falling back to a cached index.html that points at deleted JS hashes
  // is what caused the post-deploy white screen — never again.
  if (req.mode === 'navigate' || req.destination === 'document') {
    event.respondWith(
      fetch(req).catch(() => caches.match('/offline.html').then((r) => r ?? new Response('', { status: 503 }))),
    )
    return
  }

  // hashed asset files under /assets/* are immutable per Vite build.
  // Use cache-first against the runtime cache to absorb repeat hits, but
  // ALWAYS fall back to network on miss (no fabricated 404).
  if (url.pathname.startsWith('/assets/')) {
    event.respondWith(
      caches.match(req).then((cached) => {
        if (cached) return cached
        return fetch(req).then((res) => {
          if (res.ok) {
            const clone = res.clone()
            caches.open(CACHE_NAME).then((cache) => cache.put(req, clone))
          }
          return res
        })
      }),
    )
    return
  }

  // API routes: network-first, briefly cache last-good response to soften
  // transient outages. The SW cache is NOT meant as the source of truth —
  // TanStack Query is.
  if (url.pathname.startsWith('/api/')) {
    event.respondWith(
      fetch(req)
        .then((res) => {
          if (res.ok) {
            const clone = res.clone()
            caches.open(CACHE_NAME).then((cache) => cache.put(req, clone))
          }
          return res
        })
        .catch(() => caches.match(req).then((r) => r ?? new Response('', { status: 503 }))),
    )
    return
  }

  // Everything else (icons, manifest, root-level static): cache-first.
  event.respondWith(
    caches.match(req).then((cached) => {
      if (cached) return cached
      return fetch(req).catch(() => {
        if (req.mode === 'navigate') return caches.match('/offline.html')
        return new Response('', { status: 408 })
      })
    }),
  )
})

// push notifications (preserved)
self.addEventListener('push', (event) => {
  const data = event.data ? event.data.json() : { title: 'New Email', body: 'You have a new message' }
  event.waitUntil(
    self.registration.showNotification(data.title, {
      body: data.body,
      icon: '/icon.svg',
      badge: '/icon.svg',
      tag: data.tag || 'mailrs-notification',
    }),
  )
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  event.waitUntil(self.clients.openWindow('/'))
})
