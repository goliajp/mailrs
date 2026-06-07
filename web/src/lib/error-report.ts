// Fire-and-forget error reporter. Called from the top-level ErrorBoundary
// when a React render throws, so the kind of regression that previously
// only existed as a screenshot now shows up in journalctl alongside every
// other backend warning.
//
// Deliberately fail-silent:
//   - the user is already seeing the error UI; surfacing a follow-up
//     "report failed" toast would only multiply confusion.
//   - the backend endpoint is rate-limited (general bucket) so a burst of
//     reports won't break the server.

const ENDPOINT = '/api/web-errors'

type ErrorReportInput = {
  error: Error
  pathname?: string
}

export function reportRuntimeError({ error, pathname }: ErrorReportInput): void {
  const body = JSON.stringify({
    build_version: __MAILRS_VERSION__,
    error_message: error.message ?? String(error),
    error_name: error.name,
    error_stack: error.stack,
    location_pathname: pathname ?? window.location.pathname,
    occurred_at: new Date().toISOString(),
    user_agent: navigator.userAgent,
  })

  // sendBeacon is reliable even during unload; falls back to fetch.
  // keepalive=true lets the request survive a navigation away (e.g. a
  // hard reload from the ErrorBoundary's own Reload button).
  try {
    if (navigator.sendBeacon) {
      const blob = new Blob([body], { type: 'application/json' })
      const sent = navigator.sendBeacon(ENDPOINT, blob)
      if (sent) return
    }
    void fetch(ENDPOINT, {
      body,
      headers: { 'Content-Type': 'application/json' },
      keepalive: true,
      method: 'POST',
    }).catch(() => {
      // swallow — the user already sees the error UI
    })
  } catch {
    // even constructing the request failed; nothing more we can do
  }
}
