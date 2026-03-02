import { useAtom } from 'jotai'
import { useCallback, useEffect } from 'react'

import { fetchJson, postJson } from '@/lib/api'
import type { QueueEntry } from '@/lib/types'
import { queueAtom } from '@/store/admin'

const statusStyles: Record<string, string> = {
  pending: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400',
  inflight: 'bg-cyan-100 text-cyan-700 dark:bg-cyan-900/30 dark:text-cyan-400',
  delivered:
    'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400',
  failed: 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400',
  bounced:
    'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400',
}

export function AdminQueues() {
  const [queue, setQueue] = useAtom(queueAtom)

  const loadQueue = useCallback(async () => {
    try {
      const data = await fetchJson<QueueEntry[]>('/queue')
      setQueue(data)
    } catch {
      // keep current state on error
    }
  }, [setQueue])

  useEffect(() => {
    loadQueue()
    const interval = setInterval(loadQueue, 5000)
    return () => clearInterval(interval)
  }, [loadQueue])

  const handleRetry = async (id: number) => {
    await postJson(`/queue/${id}/retry`, {})
    loadQueue()
  }

  const counts = {
    pending: queue.filter((q) => q.status === 'pending').length,
    failed: queue.filter((q) => q.status === 'failed').length,
    delivered: queue.filter((q) => q.status === 'delivered').length,
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold">Outbound Queue</h2>
        <button
          onClick={loadQueue}
          className="rounded-md bg-zinc-100 px-3 py-1.5 text-sm transition-colors hover:bg-zinc-200 dark:bg-zinc-800 dark:hover:bg-zinc-700"
        >
          Refresh
        </button>
      </div>

      <div className="mb-6 flex gap-4">
        {Object.entries(counts).map(([status, count]) => (
          <div
            key={status}
            className="rounded-lg border border-zinc-200 px-4 py-3 dark:border-zinc-800"
          >
            <div className="text-2xl font-semibold tabular-nums">{count}</div>
            <div className="text-xs text-zinc-400 capitalize">{status}</div>
          </div>
        ))}
      </div>

      <div className="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-800">
        <table className="w-full text-left text-sm">
          <thead className="border-b border-zinc-200 bg-zinc-50 dark:border-zinc-800 dark:bg-zinc-900">
            <tr>
              <th className="px-4 py-2.5 font-medium">From</th>
              <th className="px-4 py-2.5 font-medium">To</th>
              <th className="px-4 py-2.5 font-medium">Domain</th>
              <th className="px-4 py-2.5 font-medium">Status</th>
              <th className="px-4 py-2.5 font-medium">Attempts</th>
              <th className="px-4 py-2.5 font-medium">Error</th>
              <th className="px-4 py-2.5 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {queue.map((item) => (
              <tr
                key={item.id}
                className="border-b border-zinc-100 last:border-0 dark:border-zinc-800/50"
              >
                <td className="px-4 py-3 font-medium">{item.sender}</td>
                <td className="px-4 py-3">{item.recipient}</td>
                <td className="px-4 py-3 text-zinc-500">{item.domain}</td>
                <td className="px-4 py-3">
                  <span
                    className={`rounded px-1.5 py-0.5 text-xs ${statusStyles[item.status] ?? ''}`}
                  >
                    {item.status}
                  </span>
                </td>
                <td className="px-4 py-3 tabular-nums">{item.attempts}</td>
                <td className="max-w-48 truncate px-4 py-3 text-xs text-zinc-400">
                  {item.last_error ?? '—'}
                </td>
                <td className="px-4 py-3 text-right">
                  {item.status === 'failed' && (
                    <button
                      onClick={() => handleRetry(item.id)}
                      className="text-xs text-blue-500 hover:text-blue-700"
                    >
                      Retry
                    </button>
                  )}
                </td>
              </tr>
            ))}
            {queue.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-8 text-center text-zinc-400">
                  Queue is empty
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}
