import type { StatusBarHealth } from './status-bar'
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

import { type Translate } from '@/i18n'

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

export function computeBackendState(
  health: null | StatusBarHealth | undefined,
  t: Translate
): IndicatorState {
  const label = t('status.backend')
  if (health == null) {
    return { dot: DOT_NEUTRAL, label, title: t('status.backendContacting') }
  }
  const status = (health.status ?? '').toLowerCase()
  if (status === 'healthy' || status === 'ok') {
    return { dot: DOT_OK, label, title: t('status.backendHealthy') }
  }
  if (status === 'degraded' || status === 'warning' || status === 'warn') {
    return { dot: DOT_WARN, label, title: t('status.backendState', { status }) }
  }
  if (status.length === 0) {
    return { dot: DOT_NEUTRAL, label, title: t('status.backendUnknown') }
  }
  return { dot: DOT_BAD, label, title: t('status.backendState', { status }) }
}

export function computeRealtimeState(state: ConnectionState, t: Translate): IndicatorState {
  const label = t('status.realtime')
  switch (state) {
    case 'connected':
      return { dot: DOT_OK, label, title: t('status.realtimeConnected') }
    case 'connecting':
      return { dot: DOT_WARN, label, title: t('status.realtimeConnecting') }
    case 'disconnected':
      return { dot: DOT_BAD, label, title: t('status.realtimeDisconnected') }
    case 'offline':
      return { dot: DOT_BAD, label, title: t('status.realtimeOffline') }
  }
}

/**
 * Map a browser pathname → the section label rendered in the status bar.
 * Kept alongside the indicator helpers so it too gets test coverage.
 */
export function sectionForPath(pathname: string, t: Translate): string {
  if (pathname.startsWith('/admin')) return t('nav.admin')
  if (pathname.startsWith('/protocol')) return t('nav.monitor')
  if (pathname.startsWith('/settings')) return t('nav.settings')
  if (pathname.startsWith('/mail')) return t('nav.mail')
  if (pathname.startsWith('/mock')) return t('nav.mock')
  return t('nav.home')
}
