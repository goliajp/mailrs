import type { Key } from '@/i18n'

// the states `GET /api/admin/queues` lists; delivered jobs are not in it
export const ALL_STATUSES = ['pending', 'retrying', 'inflight', 'failed'] as const

/** One row of `GET /api/admin/queues`: the job's stored envelope, minus the message body. */
export type QueueRow = {
  attempts?: number
  created_at?: number
  /** failed rows only: when the job failed for good, unix seconds */
  failed_at?: number
  id: number
  last_error?: null | string
  recipient: string
  sender: string
  status: QueueStatus
}

export type QueueStatus = (typeof ALL_STATUSES)[number]

export const statusLabels: Record<QueueStatus, Key> = {
  failed: 'admin.queue.status.failed',
  inflight: 'admin.queue.status.inflight',
  pending: 'admin.queue.status.pending',
  retrying: 'admin.queue.status.retrying',
}

export const statusStyles: Record<QueueStatus, string> = {
  failed: 'bg-danger/10 text-danger',
  inflight: 'bg-info/10 text-info',
  pending: 'bg-accent/10 text-accent',
  retrying: 'bg-warning/10 text-warning',
}
