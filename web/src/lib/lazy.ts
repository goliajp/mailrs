import { type ComponentType, lazy } from 'react'

// ChunkLoadError self-healing wrapper around React.lazy.
//
// After a rapid string of deploys, the browser still holding an older
// index.html may try to fetch a JS chunk whose hashed filename no longer
// exists on the server. The dynamic-import rejects, React's Suspense
// surfaces it as an error, and the user sees the top-level ErrorBoundary
// — which is exactly the "Something went wrong → click Reload → fine"
// loop that reads as a regression.
//
// Standard fix: catch the chunk-load failure and force one hard reload
// so the browser refetches the fresh index.html + chunks. A session-
// scoped sentinel prevents an infinite reload loop if the failure is
// permanent (network down, etc.).

const RELOAD_SENTINEL = 'mailrs:chunk-reload-attempted'

// Mirrors React.lazy's signature; we keep the same ComponentType<any>
// laxity so any forwardRef / generic-props component lazy-loads cleanly.
export function lazyWithReload<T extends ComponentType<any>>(
  factory: () => Promise<{ default: T }>
): ReturnType<typeof lazy<T>> {
  return lazy(() =>
    factory().catch((error) => {
      if (isChunkLoadError(error) && !sessionStorage.getItem(RELOAD_SENTINEL)) {
        sessionStorage.setItem(RELOAD_SENTINEL, '1')
        window.location.reload()
        // Never-resolving promise so React doesn't paint anything in the
        // brief window before the reload kicks in.
        return new Promise<{ default: T }>(() => {})
      }
      throw error
    })
  )
}

function isChunkLoadError(error: unknown): boolean {
  if (!(error instanceof Error)) return false
  const message = error.message || ''
  return (
    error.name === 'ChunkLoadError' ||
    /Loading chunk [\w-]+ failed/.test(message) ||
    /Failed to fetch dynamically imported module/.test(message) ||
    /error loading dynamically imported module/i.test(message)
  )
}
