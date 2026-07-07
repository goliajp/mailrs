import '@/index.css'

import { PersistQueryClientProvider } from '@tanstack/react-query-persist-client'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import { App } from '@/app'
import { persister, queryClient } from '@/lib/query-client'

// Unregister any previously-installed service worker so cached chunks
// from the old PWA cycle don't keep serving stale code after this build.
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.getRegistrations().then((regs) => {
    for (const r of regs) r.unregister()
  })
}

// v2.1 phase-8: the router itself now sits inside `<App />` as a
// `<RouterProvider>` (react-router v7 data-router API). `main.tsx`
// just wires up React Query persistence + <StrictMode>.
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PersistQueryClientProvider client={queryClient} persistOptions={{ persister }}>
      <App />
    </PersistQueryClientProvider>
  </StrictMode>
)
