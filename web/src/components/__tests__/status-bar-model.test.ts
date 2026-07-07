import { describe, expect, it } from 'vitest'

import { computeBackendState, computeRealtimeState, sectionForPath } from '../status-bar-model'

describe('computeBackendState', () => {
  it('reports the neutral "contacting…" state when the probe has not answered yet', () => {
    const s = computeBackendState(null)
    expect(s.dot).toBe('bg-fg-muted')
    expect(s.label).toBe('Backend')
    expect(s.title).toContain('contacting')
  })

  it('reports healthy when the backend says so', () => {
    const s = computeBackendState({ status: 'healthy', version: '2.0.0' })
    expect(s.dot).toBe('bg-success')
    expect(s.title).toBe('Backend: healthy')
  })

  it('accepts the alternate "ok" spelling as healthy', () => {
    expect(computeBackendState({ status: 'ok' }).dot).toBe('bg-success')
  })

  it('reports the warning colour on a degraded backend', () => {
    const s = computeBackendState({ status: 'degraded' })
    expect(s.dot).toBe('bg-warning')
    expect(s.title).toBe('Backend: degraded')
  })

  it('reports the danger colour on any unrecognized non-empty status', () => {
    const s = computeBackendState({ status: 'down' })
    expect(s.dot).toBe('bg-danger')
    expect(s.title).toBe('Backend: down')
  })

  it('falls back to neutral when the status field is empty', () => {
    // This can happen mid-flight if we get a partial response.
    expect(computeBackendState({ status: '' }).dot).toBe('bg-fg-muted')
  })
})

describe('computeRealtimeState', () => {
  it('is green when connected', () => {
    expect(computeRealtimeState('connected').dot).toBe('bg-success')
  })

  it('is amber during connection setup', () => {
    expect(computeRealtimeState('connecting').dot).toBe('bg-warning')
  })

  it('is red when offline', () => {
    expect(computeRealtimeState('offline').dot).toBe('bg-danger')
  })

  it('is red when disconnected', () => {
    expect(computeRealtimeState('disconnected').dot).toBe('bg-danger')
  })

  it('labels are stable so tooltips render deterministically', () => {
    expect(computeRealtimeState('connected').label).toBe('Realtime')
    expect(computeRealtimeState('offline').label).toBe('Realtime')
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
    ['/unknown-route', 'Home'],
  ])('%s → %s', (path, expected) => {
    expect(sectionForPath(path)).toBe(expected)
  })
})
