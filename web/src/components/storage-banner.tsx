import type { WireStorage } from '@/wire/schemas/storage'

import { useQuery } from '@tanstack/react-query'

import { useT } from '@/i18n'
import { storageKey, storageLevel } from '@/lib/storage-level'
import { wireGetStorage } from '@/wire/endpoints/storage'

/** The line above the list once the mailbox is 90% full or past its quota. */
export function StorageBanner() {
  const { data } = useQuery({
    queryKey: storageKey,
    staleTime: 5 * 60_000,
    queryFn: ({ signal }) => wireGetStorage(signal),
  })
  return <StorageNotice storage={data} />
}

export function StorageNotice({ storage }: { storage: null | undefined | WireStorage }) {
  const t = useT()
  const level = storageLevel(storage)
  if (level === 'ok' || !storage?.quota_bytes) return null
  if (level === 'over') {
    return (
      <div
        className="border-border bg-danger/10 text-danger border-b px-3 py-2 text-xs"
        role="status"
      >
        {t('storage.over')}
      </div>
    )
  }
  const pct = Math.floor((storage.used_bytes / storage.quota_bytes) * 100)
  return (
    <div
      className="border-border bg-warning/10 text-warning border-b px-3 py-2 text-xs"
      role="status"
    >
      {t('storage.near', { pct })}
    </div>
  )
}
