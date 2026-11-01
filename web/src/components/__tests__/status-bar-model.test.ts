import { describe, expect, it } from 'vitest'

import { translate } from '@/i18n'

const t: Parameters<typeof computeBackendState>[1] = (key, vars) => translate('en', key, vars)

import { computeBackendState, computeRealtimeState, sectionForPath } from '../status-bar-model'

describe('computeBackendState', () => {
  it('reports the neutral "contacting…" state when the probe has not answered yet', () => {
    const s = computeBackendState(null, t)
    expect(s.dot).toBe('bg-fg-muted')
    expect(s.label).toBe('Backend')
    expect(s.title).toContain('contacting')
  })

  it('reports healthy when the backend says so', () => {
    const s = computeBackendState({ status: 'healthy', version: '2.0.0' }, t)
    expect(s.dot).toBe('bg-success')
    expect(s.title).toBe('Backend: healthy')
  })

  it('accepts the alternate "ok" spelling as healthy', () => {
    expect(computeBackendState({ status: 'ok' }, t).dot).toBe('bg-success')
  })

  it('reports the warning colour on a degraded backend', () => {
    const s = computeBackendState({ status: 'degraded' }, t)
    expect(s.dot).toBe('bg-warning')
    expect(s.title).toBe('Backend: degraded')
  })

  it('reports the danger colour on any unrecognized non-empty status', () => {
    const s = computeBackendState({ status: 'down' }, t)
    expect(s.dot).toBe('bg-danger')
    expect(s.title).toBe('Backend: down')
  })

  it('falls back to neutral when the status field is empty', () => {
    // This can happen mid-flight if we get a partial response.
    expect(computeBackendState({ status: '' }, t).dot).toBe('bg-fg-muted')
  })
})

describe('computeRealtimeState', () => {
  it('is green when connected', () => {
    expect(computeRealtimeState('connected', t).dot).toBe('bg-success')
  })

  it('is amber during connection setup', () => {
    expect(computeRealtimeState('connecting', t).dot).toBe('bg-warning')
  })

  it('is red when offline', () => {
    expect(computeRealtimeState('offline', t).dot).toBe('bg-danger')
  })

  it('is red when disconnected', () => {
    expect(computeRealtimeState('disconnected', t).dot).toBe('bg-danger')
  })

  it('labels are stable so tooltips render deterministically', () => {
    expect(computeRealtimeState('connected', t).label).toBe('Realtime')
    expect(computeRealtimeState('offline', t).label).toBe('Realtime')
  })
})

describe('sectionForPath', () => {
  it.each([
    ['/', 'Home'],
    ['/mail', 'Mail'],
    ['/mail/thread/42', 'Mail'],
    ['/admin', 'Admin'],
    ['/admin/overview', 'Admin'],
    ['/protocol', 'Monitor'],
    ['/settings', 'Settings'],
    ['/mock', 'Mock mail'],
    ['/unknown-route', 'Home'],
  ])('%s → %s', (path, expected) => {
    expect(sectionForPath(path, t)).toBe(expected)
  })
})
