import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { mailKeys } from '@/lib/query-keys'
import { getToken } from '@/store/auth'
import { wireGetRedraft, wireListSends, wireResend } from '@/wire/endpoints/sends'

/** A failed send's compose fields, fetched when re-edit is opened. */
export function useRedraftQuery(sendId: null | string) {
  return useQuery({
    enabled: Boolean(getToken()) && Boolean(sendId),
    queryKey: mailKeys.redraft(sendId ?? ''),
    // The envelope is immutable, so this never goes stale.
    staleTime: Infinity,
    queryFn: () => wireGetRedraft(sendId ?? ''),
  })
}

/**
 * Resend a failed send.
 *
 * No optimistic row. The new send's id is derived server-side
 * (`<message_id>#r<n>`), and inventing one here to show a row sooner
 * would put a key in the list that the refetch cannot match — which is
 * the bug that made two sends render three rows on 2026-07-30.
 */
export function useResendMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (sendId: string) => wireResend(sendId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: mailKeys.all() })
    },
  })
}

/**
 * The Send list. One row per send with its delivery status, as opposed to
 * the Sent conversation axis, which lists threads and has nowhere to put
 * a status — three sends in one thread can be delivered, failed and
 * retrying at once.
 *
 * Shorter `staleTime` than the Sent list's 30 s: a row that says
 * `sending` is expected to change on its own, and a stale one reads as a
 * stuck send.
 */
export function useSendsQuery(status?: null | string) {
  return useQuery({
    enabled: Boolean(getToken()),
    queryKey: mailKeys.sends(status),
    refetchInterval: 15_000,
    staleTime: 5_000,
    queryFn: () => wireListSends(status),
  })
}
