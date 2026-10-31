import '@/index.css'

import { PersistQueryClientProvider } from '@tanstack/react-query-persist-client'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import { App } from '@/app'
import { dropOrphanedCaches, persistBuster, persister, queryClient } from '@/lib/query-client'

// Unregister any previously-installed service worker so cached chunks
// from the old PWA cycle don't keep serving stale code after this build.
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.getRegistrations().then((regs) => {
    for (const r of regs) r.unregister()
  })
}

// Before React mounts, and before the persister writes anything: every
// deploy up to 2026-08-12 stored the query cache under a key carrying
// its build id and deleted nothing, so a browser that had seen many
// releases held one blob per release. That filled localStorage, and a
// full localStorage makes `saveAuth` throw — which is why signing in
// failed with "Network error" while the login request answered 200.
dropOrphanedCaches()

// v2.1 phase-8: the router itself now sits inside `<App />` as a
// `<RouterProvider>` (react-router v7 data-router API). `main.tsx`
// just wires up React Query persistence + <StrictMode>.
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PersistQueryClientProvider
      client={queryClient}
      persistOptions={{ buster: persistBuster, persister }}
    >
      <App />
    </PersistQueryClientProvider>
  </StrictMode>
)
