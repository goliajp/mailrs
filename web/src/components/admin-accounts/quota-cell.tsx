import type { QuotaInfo } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api'
import { adminKeys } from '@/lib/query-keys'

export function QuotaCell({ address }: { address: string }) {
  const { data, isError, isPending } = useQuery({
    queryKey: [...adminKeys.accounts(), 'quota', address],
    retry: false,
    queryFn: ({ signal }) =>
      fetchJson<QuotaInfo>(`/admin/accounts/${encodeURIComponent(address)}/quota`, signal),
  })

  if (isPending) {
    return <span className="text-fg-muted text-xs">Loading...</span>
  }

  if (isError || !data) {
    return <span className="text-fg-muted text-xs">No quota set</span>
  }

  return (
    <div className="flex items-center gap-2">
      <div className="bg-border h-1.5 w-20 overflow-hidden rounded-full">
        <div className="bg-accent h-full w-0 rounded-full" />
      </div>
      <span className="text-fg-secondary text-xs">{formatBytes(data.quota_bytes)}</span>
    </div>
  )
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}
