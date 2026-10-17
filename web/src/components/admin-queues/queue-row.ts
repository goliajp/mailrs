// the states `GET /api/admin/queues` lists; finished jobs are not in it
export const ALL_STATUSES = ['pending', 'retrying', 'inflight'] as const

/** One row of `GET /api/admin/queues`: the job's stored envelope, minus the message body. */
export type QueueRow = {
  attempts?: number
  id: number
  last_error?: null | string
  recipient: string
  sender: string
  status: QueueStatus
}

export type QueueStatus = (typeof ALL_STATUSES)[number]

export const statusStyles: Record<QueueStatus, string> = {
  inflight: 'bg-info/10 text-info',
  pending: 'bg-accent/10 text-accent',
  retrying: 'bg-warning/10 text-warning',
}
