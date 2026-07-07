/**
 * Bottom status bar — the fixed 40 px strip below the AppShell.
 *
 * Design contract, so future-me (or a linter agent) doesn't erode it:
 *
 * 1. **Everything fits on one line.** The `[data-component='status-bar']`
 *    element is a rigid 40 px tall band; content that overflows wraps and
 *    reads as broken. The status bar is decorative furniture, not a
 *    content region — if a value can't fit, its dot survives, its label
 *    hides behind a tooltip (`title` attribute).
 * 2. **Every dot has a text label right next to it.** Users can't remember
 *    which colour dot means "backend" vs "realtime" — 2026-07-07 UX bug
 *    report reproduced from a shipped release. Labels are mandatory.
 * 3. **Version fields never render `undefined` / `NaN`.** If the backend
 *    hasn't answered yet, we render `contacting…` for the api version; if
 *    the vite build didn't inject `__WEB_VERSION__` (dev server), we render
 *    `dev`. The bar is a probe, so a dash means "unknown", never "0".
 * 4. **Left cluster = live signals (route, health, live connection).**
 *    **Right cluster = identity + versions.** The two clusters flex to
 *    opposite edges and never share space visually.
 *
 * Everything the pure render depends on (backend health, realtime status,
 * webapp/api version strings, section label) is derived at the caller so
 * the component itself is trivially unit-testable.
 */

import { computeBackendState, computeRealtimeState, type ConnectionState } from './status-bar-model'

export type StatusBarHealth = {
  kevy?: boolean
  pg?: boolean
  status?: string
  version?: string
}

export type StatusBarViewProps = {
  backend: null | StatusBarHealth
  identity?: string
  realtime: ConnectionState
  section: string
  webVersion: string
}

/**
 * Pure presentational component. Take everything as props so tests can
 * mount it with static values and the surrounding hooks (jotai atoms,
 * react-router location) don't have to be present.
 */
export function StatusBarView({
  backend,
  identity,
  realtime,
  section,
  webVersion,
}: StatusBarViewProps) {
  const backendState = computeBackendState(backend)
  const realtimeState = computeRealtimeState(realtime)
  const apiVersion = backend?.version ?? 'contacting…'
  return (
    <div
      aria-label="System status"
      className="text-fg-secondary flex h-full w-full items-center justify-between gap-4 px-4 text-xs"
      role="status"
    >
      <div className="flex min-w-0 items-center gap-3 overflow-hidden">
        <Indicator dot={backendState.dot} label={backendState.label} title={backendState.title} />
        <Divider />
        <Indicator
          dot={realtimeState.dot}
          label={realtimeState.label}
          title={realtimeState.title}
        />
        {backend && (typeof backend.pg === 'boolean' || typeof backend.kevy === 'boolean') && (
          <>
            <Divider />
            <span className="text-fg-muted hidden shrink-0 md:inline">
              {typeof backend.pg === 'boolean' && <>PG {backend.pg ? '✓' : '✗'}</>}
              {typeof backend.pg === 'boolean' && typeof backend.kevy === 'boolean' && ' · '}
              {typeof backend.kevy === 'boolean' && <>Kevy {backend.kevy ? '✓' : '✗'}</>}
            </span>
          </>
        )}
        <Divider />
        <span className="shrink-0">{section}</span>
      </div>
      <div className="flex shrink-0 items-center gap-3">
        {identity && (
          <>
            <span className="text-fg-muted hidden truncate sm:inline">{identity}</span>
            <Divider className="hidden sm:inline" />
          </>
        )}
        <span
          className="whitespace-nowrap"
          title={`webapp: ${webVersion} · backend: ${apiVersion}`}
        >
          <span className="text-fg-muted">web </span>
          <span data-testid="web-version">{webVersion}</span>
          <span className="text-border-strong"> · </span>
          <span className="text-fg-muted">api </span>
          <span data-testid="api-version">{apiVersion}</span>
        </span>
      </div>
    </div>
  )
}

function Divider({ className = '' }: { className?: string }) {
  return <span className={`text-border-strong shrink-0 ${className}`}>·</span>
}

function Indicator({ dot, label, title }: { dot: string; label: string; title: string }) {
  return (
    <span className="flex shrink-0 items-center gap-1.5" title={title}>
      <span aria-hidden="true" className={`inline-block h-2.5 w-2.5 rounded-full ${dot}`} />
      <span>{label}</span>
    </span>
  )
}
