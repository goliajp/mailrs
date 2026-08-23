import { useQuery } from '@tanstack/react-query'

import { settingsKeys } from '@/lib/query-keys'
import { wireListExternalAccounts } from '@/wire/endpoints/external-accounts'

/**
 * Which colour marks which mailbox, on a merged list.
 *
 * Several mailboxes read into one inbox is the point of connecting
 * them — and a merged list where every row looks the same is a list
 * where nobody can tell whose mail they are reading. The server picks
 * the colour so all three clients agree; nothing was drawing it.
 *
 * Empty until there is more than one mailbox: a mark that is on every
 * row and means nothing is furniture.
 */
export function useAccountColours(): Map<string, string> {
  const accountsQuery = useQuery({
    queryKey: settingsKeys.externalAccounts(),
    staleTime: 60_000,
    queryFn: () => wireListExternalAccounts(),
  })
  const connected = accountsQuery.data ?? []
  if (connected.length === 0) return new Map()
  return new Map(connected.filter((a) => a.colour).map((a) => [a.id, a.colour as string]))
}
