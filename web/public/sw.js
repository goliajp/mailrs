// Kill-switch service worker.
//
// The PWA service worker was removed in 5a8dee3 (2026-06-08), but clients
// that installed an OLD cache-first SW (the pre-v1.7.99 generation that
// cached index.html) are deadlocked: the stale SW serves a cached
// index.html whose hashed JS chunks no longer exist on the server, the
// page white-screens, and the unregister call shipped in the new bundle
// never executes because the new bundle never loads.
//
// Serving this file at the old registration URL breaks the deadlock via
// the browser's SW update cycle: on the next navigation (or within 24h),
// the browser byte-compares /sw.js, installs this version, and activate
// nukes every cache, unregisters, and reloads all open clients straight
// from the network. Clients without a SW never request this file.
//
// Keep this file deployed permanently — it is inert after one activation
// and guarantees any straggler ever returning gets unstuck.

self.addEventListener('install', () => {
  self.skipWaiting()
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const keys = await caches.keys()
      await Promise.all(keys.map((k) => caches.delete(k)))
      await self.registration.unregister()
      const clients = await self.clients.matchAll({ type: 'window' })
      for (const client of clients) {
        client.navigate(client.url)
      }
    })()
  )
})

// no fetch handler — every request passes straight through to the network
