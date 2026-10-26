import type { AdminFlow } from '@/wire/schemas/admin-flow'

import { count, uptime } from './format'

export type Item = { text: string; to?: string }

export type Lane = { name: string; note: string; stages: Stage[] }

export type Stage = {
  /** what the stage number counts */
  caption: string
  details: string[]
  name: string
  to?: string
  tone: Tone
  value: null | number
}

export type Tone = 'danger' | 'plain' | 'warning'

// a spool that has not drained in a few ticks is worth a look
const SPOOL_WARN = 20
const MOCK_FULL = 0.8

/** Only what needs a person; empty when all is well. */
export function attentionItems(flow: AdminFlow, kevyUp: boolean | null): Item[] {
  const items: Item[] = []
  if (kevyUp === false) {
    items.push({ text: 'kevy is not answering: sessions, queues and settings are unavailable' })
  }
  // older failures are history: their bounces went out long ago
  if (flow.outbound !== null && flow.outbound.failed_recent > 0) {
    items.push({
      text: `${plural(flow.outbound.failed_recent, 'outbound message')} failed for good in the last 7 days`,
      to: '/admin/queues?status=failed',
    })
  }
  if (flow.spool_backlog !== null && flow.spool_backlog >= SPOOL_WARN) {
    items.push({
      text: `${plural(flow.spool_backlog, 'message')} waiting in the spool; check that fastcore is delivering`,
    })
  }
  if (flow.receiving === null) {
    items.push({ text: 'The receiver has not reported its counters yet', to: '/protocol' })
  }
  if (flow.mock !== null && flow.mock.bytes >= flow.mock.max_bytes * MOCK_FULL) {
    items.push({ text: 'Mock mail storage is over 80% full', to: '/admin/mock' })
  }
  return items
}

/** Sizes in the unit that reads best: 877 B, 23.4 MB, 2 GB. */
export function bytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`
  return `${Number((n / 1024 ** 3).toFixed(1))} GB`
}

/**
 * Two lanes, because they are two flows: mail arriving from outside is
 * spooled and delivered here; mail people send waits in the outbound
 * queue until a remote server takes it or it fails for good.
 */
export function lanesOf(flow: AdminFlow): Lane[] {
  return [
    { name: 'Inbound', note: 'Mail arriving from other servers', stages: inboundStages(flow) },
    { name: 'Outbound', note: 'Mail people here send', stages: outboundStages(flow) },
  ]
}

function failedTone(failed: null | number): Tone {
  if (failed !== null && failed > 0) return 'danger'
  return 'plain'
}

function inboundStages(flow: AdminFlow): Stage[] {
  const stages: Stage[] = [
    {
      caption: 'connections open now',
      details: receivingDetails(flow),
      name: 'Receiving',
      to: '/protocol',
      tone: 'plain',
      value: nullable(flow.receiving, (x) => x.active_connections),
    },
    {
      caption: 'waiting to be delivered',
      details: ['Delivered every 15 seconds'],
      name: 'Spool',
      tone: spoolTone(flow.spool_backlog),
      value: flow.spool_backlog,
    },
  ]
  if (flow.mock !== null) {
    stages.push({
      caption: 'test messages kept now',
      details: [
        `${bytes(flow.mock.bytes)} of ${bytes(flow.mock.max_bytes)} used`,
        `${count(flow.mock.received)} received since mock mail started`,
      ],
      name: 'Mock mail',
      to: '/admin/mock',
      tone: 'plain',
      value: flow.mock.stored,
    })
  }
  return stages
}

function nullable<T>(value: null | T, pick: (v: T) => number): null | number {
  if (value === null) return null
  return pick(value)
}

function outboundStages(flow: AdminFlow): Stage[] {
  const out = flow.outbound
  return [
    {
      caption: 'waiting to be sent',
      details: [`${count(nullable(out, (x) => x.scheduled))} scheduled for later`],
      name: 'Queued',
      to: '/admin/queues',
      tone: 'plain',
      value: nullable(out, (x) => x.queued),
    },
    {
      caption: 'deferred by the remote server',
      details: ['Retried with growing gaps for up to 5 days'],
      name: 'Retrying',
      to: '/admin/queues',
      tone: 'plain',
      value: nullable(out, (x) => x.retrying),
    },
    {
      caption: 'failed for good in the last 7 days',
      details: [
        `${count(nullable(out, (x) => x.failed))} in all`,
        'Each one sent a bounce to its sender',
      ],
      name: 'Failed',
      to: '/admin/queues?status=failed',
      tone: failedTone(nullable(out, (x) => x.failed_recent)),
      value: nullable(out, (x) => x.failed_recent),
    },
  ]
}

function plural(n: number, noun: string): string {
  if (n === 1) return `1 ${noun}`
  return `${count(n)} ${noun}s`
}

function receivingDetails(flow: AdminFlow): string[] {
  const r = flow.receiving
  if (r === null) return ['The receiver has not reported yet']
  return [
    `${count(r.spooled)} accepted in the ${uptime(r.uptime_secs)} since the receiver started`,
    `${count(r.connections)} connections in that time`,
  ]
}

function spoolTone(backlog: null | number): Tone {
  if (backlog !== null && backlog >= SPOOL_WARN) return 'warning'
  return 'plain'
}
