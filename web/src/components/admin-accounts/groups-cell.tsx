import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { adminKeys } from '@/lib/query-keys'
import { adminStringListGet } from '@/wire/endpoints/admin'

export function GroupsCell({ address }: { address: string }) {
  const [open, setOpen] = useState(false)

  const { data, isPending } = useQuery({
    enabled: open,
    queryKey: [...adminKeys.accounts(), 'groups', address],
    queryFn: ({ signal }) =>
      adminStringListGet(`/admin/accounts/${encodeURIComponent(address)}/groups`, signal),
  })

  if (!open) {
    return (
      <button className="text-accent text-xs hover:opacity-80" onClick={() => setOpen(true)}>
        Groups
      </button>
    )
  }

  if (isPending) {
    return <span className="text-fg-muted text-xs">Loading...</span>
  }

  const groups = data ?? []

  return (
    <div className="flex flex-wrap items-center gap-1">
      {groups.length > 0 ? (
        groups.map((g) => (
          <span className="bg-accent/10 text-accent rounded-full px-2 py-0.5 text-xs" key={g}>
            {g}
          </span>
        ))
      ) : (
        <span className="text-fg-muted text-xs">No groups</span>
      )}
      <button
        className="text-fg-muted hover:text-fg-secondary ml-1 text-xs"
        onClick={() => setOpen(false)}
      >
        Close
      </button>
    </div>
  )
}
