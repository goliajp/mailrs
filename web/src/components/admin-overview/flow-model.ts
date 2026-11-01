import type { Translate } from './format'
import type { Key } from '@/i18n'
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
export function attentionItems(flow: AdminFlow, kevyUp: boolean | null, t: Translate): Item[] {
  const items: Item[] = []
  if (kevyUp === false) {
    items.push({ text: t('admin.flow.attn.kevy') })
  }
  // older failures are history: their bounces went out long ago
  if (flow.outbound !== null && flow.outbound.failed_recent > 0) {
    items.push({
      text: plural(
        flow.outbound.failed_recent,
        'admin.flow.attn.failedOne',
        'admin.flow.attn.failedMany',
        t
      ),
      to: '/admin/queues?status=failed',
    })
  }
  if (flow.spool_backlog !== null && flow.spool_backlog >= SPOOL_WARN) {
    items.push({
      text: plural(flow.spool_backlog, 'admin.flow.attn.spoolOne', 'admin.flow.attn.spoolMany', t),
    })
  }
  if (flow.receiving === null) {
    items.push({ text: t('admin.flow.attn.noReceiver'), to: '/protocol' })
  }
  if (flow.mock !== null && flow.mock.bytes >= flow.mock.max_bytes * MOCK_FULL) {
    items.push({ text: t('admin.flow.attn.mockFull'), to: '/admin/mock' })
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
export function lanesOf(flow: AdminFlow, t: Translate): Lane[] {
  return [
    {
      name: t('admin.flow.inbound'),
      note: t('admin.flow.inboundNote'),
      stages: inboundStages(flow, t),
    },
    {
      name: t('admin.flow.outbound'),
      note: t('admin.flow.outboundNote'),
      stages: outboundStages(flow, t),
    },
  ]
}

function failedTone(failed: null | number): Tone {
  if (failed !== null && failed > 0) return 'danger'
  return 'plain'
}

function inboundStages(flow: AdminFlow, t: Translate): Stage[] {
  const stages: Stage[] = [
    {
      caption: t('admin.flow.receiving.caption'),
      details: receivingDetails(flow, t),
      name: t('admin.flow.receiving'),
      to: '/protocol',
      tone: 'plain',
      value: nullable(flow.receiving, (x) => x.active_connections),
    },
    {
      caption: t('admin.flow.spool.caption'),
      details: [t('admin.flow.spool.detail')],
      name: t('admin.flow.spool'),
      tone: spoolTone(flow.spool_backlog),
      value: flow.spool_backlog,
    },
  ]
  if (flow.mock !== null) {
    stages.push({
      caption: t('admin.flow.mock.caption'),
      details: [
        t('admin.flow.mock.used', {
          max: bytes(flow.mock.max_bytes),
          used: bytes(flow.mock.bytes),
        }),
        t('admin.flow.mock.received', { n: count(flow.mock.received) }),
      ],
      name: t('admin.audit.group.mock'),
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

function outboundStages(flow: AdminFlow, t: Translate): Stage[] {
  const out = flow.outbound
  return [
    {
      caption: t('admin.flow.queued.caption'),
      details: [t('admin.flow.queued.scheduled', { n: count(nullable(out, (x) => x.scheduled)) })],
      name: t('admin.flow.queued'),
      to: '/admin/queues',
      tone: 'plain',
      value: nullable(out, (x) => x.queued),
    },
    {
      caption: t('admin.flow.retrying.caption'),
      details: [t('admin.flow.retrying.detail')],
      name: t('admin.flow.retrying'),
      to: '/admin/queues',
      tone: 'plain',
      value: nullable(out, (x) => x.retrying),
    },
    {
      caption: t('admin.flow.failed.caption'),
      details: [
        t('admin.flow.failed.all', { n: count(nullable(out, (x) => x.failed)) }),
        t('admin.flow.failed.bounce'),
      ],
      name: t('admin.flow.failed'),
      to: '/admin/queues?status=failed',
      tone: failedTone(nullable(out, (x) => x.failed_recent)),
      value: nullable(out, (x) => x.failed_recent),
    },
  ]
}

function plural(n: number, one: Key, many: Key, t: Translate): string {
  if (n === 1) return t(one)
  return t(many, { n: count(n) })
}

function receivingDetails(flow: AdminFlow, t: Translate): string[] {
  const r = flow.receiving
  if (r === null) return [t('admin.flow.receiving.none')]
  return [
    t('admin.flow.receiving.accepted', {
      n: count(r.spooled),
      uptime: uptime(r.uptime_secs, t),
    }),
    t('admin.flow.receiving.connections', { n: count(r.connections) }),
  ]
}

function spoolTone(backlog: null | number): Tone {
  if (backlog !== null && backlog >= SPOOL_WARN) return 'warning'
  return 'plain'
}
