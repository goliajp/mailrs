/**
 * Pure logic behind the status bar: no React, no hooks, no DOM.
 *
 * The two indicators (backend health, realtime connection) each collapse
 * a fuzzy input into a small deterministic bag:
 *   - the tailwind class for the dot colour
 *   - the short label rendered next to it
 *   - the long-form title used for the tooltip
 *
 * These functions ship as the primary unit-test surface for the status
 * bar. If a colour changes, the test asserts it. If a wording changes,
 * the test asserts it. React rendering is a thin wrapper around them.
 */

import type { StatusBarHealth } from './status-bar'

export type ConnectionState = 'connected' | 'connecting' | 'disconnected' | 'offline'

export type IndicatorState = {
  dot: string
  label: string
  title: string
}

/** Idle "nothing to report yet" dot — dim grey so a load doesn't flash red. */
const DOT_NEUTRAL = 'bg-fg-muted'
const DOT_OK = 'bg-success'
const DOT_WARN = 'bg-warning'
const DOT_BAD = 'bg-danger'

export function computeBackendState(health: null | StatusBarHealth | undefined): IndicatorState {
  if (health == null) {
    return { dot: DOT_NEUTRAL, label: 'Backend', title: 'Backend: contacting…' }
  }
  const status = (health.status ?? '').toLowerCase()
  if (status === 'healthy' || status === 'ok') {
    return { dot: DOT_OK, label: 'Backend', title: 'Backend: healthy' }
  }
  if (status === 'degraded' || status === 'warning' || status === 'warn') {
    return { dot: DOT_WARN, label: 'Backend', title: `Backend: ${status}` }
  }
  if (status.length === 0) {
    return { dot: DOT_NEUTRAL, label: 'Backend', title: 'Backend: unknown' }
  }
  return { dot: DOT_BAD, label: 'Backend', title: `Backend: ${status}` }
}

export function computeRealtimeState(state: ConnectionState): IndicatorState {
  switch (state) {
    case 'connected':
      return { dot: DOT_OK, label: 'Realtime', title: 'Realtime updates: connected' }
    case 'connecting':
      return { dot: DOT_WARN, label: 'Realtime', title: 'Realtime updates: connecting' }
    case 'disconnected':
    case 'offline':
      return { dot: DOT_BAD, label: 'Realtime', title: `Realtime updates: ${state}` }
  }
}

/**
 * Map a browser pathname → the section label rendered in the status bar.
 * Kept alongside the indicator helpers so it too gets test coverage.
 */
export function sectionForPath(pathname: string): string {
  if (pathname.startsWith('/admin')) return 'Admin'
  if (pathname.startsWith('/protocol')) return 'Monitor'
  if (pathname.startsWith('/settings')) return 'Settings'
  if (pathname.startsWith('/mail')) return 'Mail'
  return 'Home'
}
