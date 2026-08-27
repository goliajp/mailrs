import { describe, expect, it } from 'vitest'

import { activeLabel, serviceLabel, stateDotClass } from '@/lib/status-label'

/**
 * A dot never carries the state on its own.
 *
 * Two places did: the admin accounts table (green against grey at 8×8,
 * with a `title` on a span nobody can focus) and the service pills
 * (green against red — the pair about 8% of men cannot separate, on
 * the question you can least afford to guess at).
 */
describe('status labels', () => {
  it('says both states of an account in words', () => {
    expect(activeLabel(true)).toBe('Active')
    expect(activeLabel(false)).toBe('Inactive')
    expect(activeLabel(true)).not.toBe(activeLabel(false))
  })

  it('says both states of a service in words', () => {
    expect(serviceLabel(true)).toBe('up')
    expect(serviceLabel(false)).toBe('down')
  })

  it('gives the two states different colours, and the caller picks the off one', () => {
    expect(stateDotClass(true, 'bg-border')).toContain('bg-success')
    expect(stateDotClass(false, 'bg-border')).toContain('bg-border')
    expect(stateDotClass(false, 'bg-danger')).toContain('bg-danger')
    // Same geometry either way — a state should not change the size.
    expect(stateDotClass(true, 'bg-border')).toContain('h-2 w-2')
    expect(stateDotClass(false, 'bg-border')).toContain('h-2 w-2')
  })
})
