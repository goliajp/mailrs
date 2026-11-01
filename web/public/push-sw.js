// Web Push service worker: shows "you have mail" and opens the conversation
// when the notification is clicked. Nothing else.
//
// Registered with its own scope (/push-sw/), so it never controls a page and
// never takes the place of /sw.js, the kill switch for the old caching
// worker. No fetch handler: every request goes to the network.

const HIDDEN_TITLE = {
  en: 'New mail',
  ja: '新着メール',
  zh: '新邮件',
}

function hiddenTitle() {
  const lang = (self.navigator && self.navigator.language) || 'en'
  return HIDDEN_TITLE[lang.slice(0, 2)] || HIDDEN_TITLE.en
}

// the server's payload, as the notification to show
function notificationFor(data) {
  const tag = data.thread_id
  const meta = { notification_id: data.notification_id, thread_id: data.thread_id }
  if (data.hidden) {
    return { options: { data: meta, tag }, title: hiddenTitle() }
  }
  return { options: { body: data.subject || '', data: meta, tag }, title: data.sender || '' }
}

function threadUrl(threadId) {
  return '/mail?thread=' + encodeURIComponent(threadId) + '&view=thread'
}

self.addEventListener('install', () => {
  self.skipWaiting()
})

self.addEventListener('push', (event) => {
  if (!event.data) return
  const shown = notificationFor(event.data.json())
  event.waitUntil(self.registration.showNotification(shown.title, shown.options))
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  const data = event.notification.data || {}
  if (!data.thread_id) return
  const url = threadUrl(data.thread_id)
  event.waitUntil(
    (async () => {
      // a lost click count must not keep the conversation from opening
      await fetch('/api/push/clicked', {
        body: JSON.stringify({ notification_id: data.notification_id || '' }),
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        method: 'POST',
      }).catch(() => undefined)
      const windows = await self.clients.matchAll({ includeUncontrolled: true, type: 'window' })
      const open = windows.find((w) => new URL(w.url).origin === self.location.origin)
      if (open) {
        await open.focus()
        await open.navigate(url)
        return
      }
      await self.clients.openWindow(url)
    })()
  )
})
