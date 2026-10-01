import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { mailKeys } from '@/lib/query-keys'
import { getToken } from '@/store/auth'
import {
  wireCancelSend,
  wireDeleteSend,
  wireGetRedraft,
  wireListSends,
  wireResend,
} from '@/wire/endpoints/sends'

/**
 * Stop a send that has not gone out.
 *
 * Invalidates rather than patching a row optimistically: the answer is
 * three numbers, and one of them — `already_delivered` — means the
 * opposite of what the button implies. Showing "Cancelled" before the
 * server has said so would be showing it for mail that arrived.
 */
export function useCancelSendMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (sendId: string) => wireCancelSend(sendId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: mailKeys.sends() })
    },
  })
}

/**
 * Delete a finished send. The Send list joins two sources and the
 * server removes both, so everything mail-shaped is refetched.
 */
export function useDeleteSendMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (sendId: string) => wireDeleteSend(sendId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: mailKeys.all() })
    },
  })
}

/** A failed send's compose fields, fetched when re-edit is opened. */
export function useRedraftQuery(sendId: null | string) {
  return useQuery({
    enabled: Boolean(getToken()) && Boolean(sendId),
    queryKey: mailKeys.redraft(sendId ?? ''),
    // `Infinity`, and this one earns it: the handler reads the bytes
    // of the envelope that was submitted
    // (`handlers/sends/redraft.rs::envelope_bytes`), and nothing
    // rewrites a send that already failed — a retry makes a new send.
    // writers-checked: 2026-09-01 — every caller of the send store,
    // on the thread query and on the fraud verdict turned out to be
    // false. `staleTime: Infinity` is a claim about the writers, so
    // it is only ever as good as having enumerated them.
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
export function useSendsQuery(status?: null | string, enabled: boolean = true) {
  return useQuery({
    enabled: enabled && Boolean(getToken()),
    queryKey: mailKeys.sends(status),
    refetchInterval: 15_000,
    staleTime: 5_000,
    queryFn: () => wireListSends(status),
  })
}
