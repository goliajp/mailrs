import { useQuery } from '@tanstack/react-query'

import { adminKeys } from '@/lib/query-keys'
import { adminObjectGet } from '@/wire/endpoints/admin'

import { quotaLabel } from './quota-label'

// Backend: crates/webapi/src/handlers/admin_directory/accounts.rs — fn get_account_quota
type QuotaResponse = { quota_bytes?: null | number }

export function QuotaCell({ address }: { address: string }) {
  const { data, isPending } = useQuery({
    queryKey: [...adminKeys.accounts(), 'quota', address],
    retry: false,
    queryFn: ({ signal }) =>
      adminObjectGet<QuotaResponse>(`/admin/accounts/${encodeURIComponent(address)}/quota`, signal),
  })

  if (isPending) {
    return <span className="text-fg-muted text-xs">Loading...</span>
  }

  return <span className="text-fg-secondary text-xs">{quotaLabel(data?.quota_bytes)}</span>
}
