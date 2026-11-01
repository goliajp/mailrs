import { useQuery } from '@tanstack/react-query'

import { useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminStringListGet } from '@/wire/endpoints/admin'

/** The permission groups an account belongs to, under its row. */
export function GroupsPanel({ address }: { address: string }) {
  const t = useT()
  const { data, isPending } = useQuery({
    queryKey: [...adminKeys.accounts(), 'groups', address],
    queryFn: ({ signal }) =>
      adminStringListGet(`/admin/accounts/${encodeURIComponent(address)}/groups`, signal),
  })

  if (isPending) {
    return <span className="text-fg-muted text-xs">{t('admin.loading')}</span>
  }

  const groups = data ?? []
  if (groups.length === 0) {
    return <span className="text-fg-muted text-xs">{t('admin.accounts.noGroups')}</span>
  }

  return (
    <div className="flex flex-wrap items-center gap-1">
      <span className="text-fg-secondary mr-1 text-xs">{t('admin.permissionGroups')}</span>
      {groups.map((g) => (
        <span className="bg-accent/10 text-accent rounded-full px-2 py-0.5 text-xs" key={g}>
          {g}
        </span>
      ))}
    </div>
  )
}
