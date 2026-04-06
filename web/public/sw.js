const CACHE_NAME = 'mailrs-v1'
const SHELL_ASSETS = [
  '/',
  '/icon.svg',
  '/icon-192.png',
  '/icon-512.png',
  '/offline.html',
]

// install: cache app shell assets
self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(SHELL_ASSETS))
  )
  self.skipWaiting()
})

// activate: clean old caches
self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k)))
    )
  )
  self.clients.claim()
})

// fetch: network-first for API, cache-first for shell
self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url)

  // never cache non-GET requests
  if (event.request.method !== 'GET') return

  // API routes: network first, fall back to cache
  if (url.pathname.startsWith('/api/')) {
    event.respondWith(
      fetch(event.request)
        .then((res) => {
          const clone = res.clone()
          caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone))
          return res
        })
        .catch(() => caches.match(event.request))
    )
    return
  }

  // app shell: cache first, fall back to network, then offline page
  event.respondWith(
    caches.match(event.request).then((cached) => {
      if (cached) return cached
      return fetch(event.request).catch(() => {
        // for navigation requests, serve offline page
        if (event.request.mode === 'navigate') {
          return caches.match('/offline.html')
        }
        return new Response('', { status: 408 })
      })
    })
  )
})

// push notifications (preserved from original)
self.addEventListener('push', (event) => {
  const data = event.data ? event.data.json() : { title: 'New Email', body: 'You have a new message' }
  event.waitUntil(
    self.registration.showNotification(data.title, {
      body: data.body,
      icon: '/icon.svg',
      badge: '/icon.svg',
      tag: data.tag || 'mailrs-notification',
    })
  )
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  event.waitUntil(self.clients.openWindow('/'))
})
