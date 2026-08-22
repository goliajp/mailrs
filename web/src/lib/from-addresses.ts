import { useQuery } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'

import { settingsKeys } from '@/lib/query-keys'
import { authAtom } from '@/store/auth'
import { wireListExternalAccounts } from '@/wire/endpoints/external-accounts'

/** One address a message can leave by. */
export type FromAddress = { accountId: string; address: string; label: string }

/**
 * The address a reply should leave by, given where the mail arrived.
 *
 * Not "the account you signed in as". Replying to mail that arrived at
 * a connected Gmail must go out through that Gmail: a reply from
 * somewhere else lands in the thread as a stranger, and half the time
 * the recipient's provider refuses it outright.
 *
 * Falls back to this server's own address when the conversation came
 * from an account that is gone or cannot send — replying from
 * somewhere beats a compose window that will not send.
 */
export function replyFromFor(
  accountId: null | string | undefined,
  addresses: FromAddress[]
): string {
  const match = addresses.find((a) => a.accountId === (accountId ?? ''))
  return match?.address ?? addresses[0]?.address ?? ''
}

/**
 * Every address this person can send as, this server's own first.
 *
 * An account whose credential was refused is left out: choosing it
 * would produce a message that cannot be sent, and offering a choice
 * that fails is worse than not offering it.
 */
export function useFromAddresses(): { addresses: FromAddress[] } {
  const auth = useAtomValue(authAtom)
  const accountsQuery = useQuery({
    queryKey: settingsKeys.externalAccounts(),
    staleTime: 60_000,
    queryFn: () => wireListExternalAccounts(),
  })
  const own = auth?.address ?? ''
  const addresses: FromAddress[] = own ? [{ accountId: '', address: own, label: own }] : []
  for (const a of accountsQuery.data ?? []) {
    if (a.state === 'needs_auth') continue
    addresses.push({
      accountId: a.id,
      address: a.email,
      label:
        a.display_name && a.display_name !== a.email ? `${a.display_name} · ${a.email}` : a.email,
    })
  }
  return { addresses }
}
