import type { ComposerStart } from './start'

import { useQuery } from '@tanstack/react-query'
import { useEffect, useRef } from 'react'

import { settingsKeys } from '@/lib/query-keys'
import { splitAddressList } from '@/lib/recipients'
import { wireListMaskedEmails } from '@/wire/endpoints/masked-email'

import { isReplyLike, replyMaskOf } from './start'

/**
 * A reply to mail sent to one of the user's masked addresses goes out from
 * that mask, and the mask leaves the recipients: once, when the masks are
 * known, unless the From has been changed already. Returns the mask, to be
 * offered in the From picker; `''` when there is none.
 */
export function useReplyMask(
  start: ComposerStart,
  from: string,
  own: string,
  fields: {
    setCc: (fn: (v: string) => string) => void
    setFrom: (v: string) => void
    setTo: (fn: (v: string) => string) => void
  }
): string {
  const { data } = useQuery({
    enabled: isReplyLike(start),
    queryKey: settingsKeys.maskedEmails(),
    queryFn: ({ signal }) => wireListMaskedEmails(signal),
  })
  const live = (data ?? []).filter((m) => m.state === 'enabled' || m.state === 'pending')
  const masks = live.map((m) => m.email)
  const mask = replyMaskOf(start, masks)
  const applied = useRef(false)
  const { setCc, setFrom, setTo } = fields
  useEffect(() => {
    if (applied.current || mask === '') return
    applied.current = true
    if (from !== '' && from.toLowerCase() !== own.toLowerCase()) return
    setFrom(mask)
    setTo((v) => without(v, mask))
    setCc((v) => without(v, mask))
  }, [mask, from, own, setCc, setFrom, setTo])
  return mask
}

/** `list` without `address`, as the comma-separated field holds it. */
function without(list: string, address: string): string {
  return splitAddressList(list)
    .filter((a) => !a.toLowerCase().includes(address.toLowerCase()))
    .join(', ')
}
