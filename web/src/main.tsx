import '@/index.css'

import { PersistQueryClientProvider } from '@tanstack/react-query-persist-client'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'

import { App } from '@/app'
import { reportRuntimeError } from '@/lib/error-report'
import { persister, queryClient } from '@/lib/query-client'

// Catch the failures that don't pass through React: native event handlers,
// async functions called outside a component tree, dynamic-import failures
// that bypass our lazyWithReload guard.
window.addEventListener('error', (e) => {
  if (e.error instanceof Error) reportRuntimeError({ error: e.error })
})
window.addEventListener('unhandledrejection', (e) => {
  const reason = e.reason
  if (reason instanceof Error) reportRuntimeError({ error: reason })
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PersistQueryClientProvider client={queryClient} persistOptions={{ persister }}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </PersistQueryClientProvider>
  </StrictMode>
)

// register service worker for offline support and push notifications
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.register('/sw.js').then((reg) => {
    reg.addEventListener('updatefound', () => {
      const newWorker = reg.installing
      if (!newWorker) return
      newWorker.addEventListener('statechange', () => {
        if (newWorker.state === 'activated' && navigator.serviceWorker.controller) {
          // new version available — user will get it on next reload
          console.info('[SW] new version available')
        }
      })
    })
  })
}
