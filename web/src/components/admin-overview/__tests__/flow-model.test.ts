import type { AdminFlow } from '@/wire/schemas/admin-flow'

import { describe, expect, it } from 'vitest'

import { attentionItems, bytes, lanesOf } from '../flow-model'

const OUT = { failed: 0, queued: 2, retrying: 1, scheduled: 0 }

const FLOW: AdminFlow = {
  mock: { bytes: 877, max_bytes: 2_147_483_648, received: 5, stored: 2 },
  outbound: OUT,
  receiving: { active_connections: 1, connections: 126, spooled: 10, uptime_secs: 11_060 },
  spool_backlog: 0,
}

function stage(flow: AdminFlow, lane: string, name: string) {
  return lanesOf(flow)
    .find((l) => l.name === lane)
    ?.stages.find((st) => st.name === name)
}

describe('flow lanes', () => {
  it('keeps inbound and outbound apart, with mock mail only when it exists', () => {
    const lanes = lanesOf(FLOW)
    expect(lanes.map((l) => l.stages.map((st) => st.name))).toEqual([
      ['Receiving', 'Spool', 'Mock mail'],
      ['Queued', 'Retrying', 'Failed'],
    ])
    expect(lanesOf({ ...FLOW, mock: null })[0].stages.map((st) => st.name)).toEqual([
      'Receiving',
      'Spool',
    ])
  })

  it('leaves an unreadable stage unknown rather than zero', () => {
    const blind = { ...FLOW, outbound: null, receiving: null, spool_backlog: null }
    expect(stage(blind, 'Inbound', 'Receiving')?.value).toBeNull()
    expect(stage(blind, 'Inbound', 'Spool')?.value).toBeNull()
    expect(stage(blind, 'Outbound', 'Failed')?.value).toBeNull()
    expect(stage(blind, 'Outbound', 'Queued')?.details).toEqual(['— scheduled for later'])
  })

  it('marks failures and a backed-up spool', () => {
    const busy = { ...FLOW, outbound: { ...OUT, failed: 4 }, spool_backlog: 25 }
    expect(stage(busy, 'Outbound', 'Failed')?.tone).toBe('danger')
    expect(stage(busy, 'Inbound', 'Spool')?.tone).toBe('warning')
  })

  it('writes sizes in the unit that reads best', () => {
    expect(bytes(877)).toBe('877 B')
    expect(bytes(23_400_000)).toBe('22.3 MB')
    expect(bytes(2_147_483_648)).toBe('2 GB')
  })
})

describe('attention', () => {
  it('is empty when all is well', () => {
    expect(attentionItems(FLOW, true)).toEqual([])
  })

  it('lists what needs a person', () => {
    const items = attentionItems(
      { ...FLOW, outbound: { ...OUT, failed: 4 }, receiving: null },
      false
    ).map((i) => i.text)
    expect(items).toHaveLength(3)
    expect(items[0]).toContain('kevy')
    expect(items[1]).toContain('4 outbound messages failed')
    expect(attentionItems({ ...FLOW, outbound: { ...OUT, failed: 1 } }, true)[0].text).toBe(
      '1 outbound message failed for good'
    )
  })
})
