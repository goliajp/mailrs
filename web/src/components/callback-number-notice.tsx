import type { GivenNumber, ThreadMessage } from '@/lib/types'

import { useQuery } from '@tanstack/react-query'
import { Phone } from 'lucide-react'
import { useMemo } from 'react'

import { useT } from '@/i18n'
import { extractEmail } from '@/lib/avatar'
import { newCallbackNumbers } from '@/lib/callback-numbers'
import { wireCallbackNumbers } from '@/wire/endpoints/callback-numbers'

const NO_HISTORY: readonly GivenNumber[] = []

/**
 * The phone numbers a message gives that its sender has not given the
 * reader before, in this conversation or any other. A notice, not a
 * verdict: a number proves nothing about who answers it, and invoices and
 * receipts carry them too.
 */
export function CallbackNumberNotice({
  conversation,
  message,
}: {
  conversation: readonly ThreadMessage[]
  message: ThreadMessage
}) {
  const t = useT()
  const sender = extractEmail(message.sender)
  const earlier = useMemo(
    () => conversation.slice(0, conversation.indexOf(message)),
    [message, conversation]
  )
  // the history is asked for only when this conversation leaves a number new
  const unseenHere = useMemo(
    () => newCallbackNumbers(message, earlier).length > 0,
    [message, earlier]
  )
  const history = useQuery({
    enabled: unseenHere && sender.includes('@'),
    queryKey: ['callback-numbers', sender],
    queryFn: () => wireCallbackNumbers(sender),
  })
  const given = history.data?.numbers ?? NO_HISTORY
  const numbers = useMemo(
    () => newCallbackNumbers(message, earlier, given),
    [message, earlier, given]
  )
  if (history.isLoading || numbers.length === 0) return null
  return (
    <div className="text-fg-muted flex items-start gap-2 px-4 py-2 text-xs" data-callback-notice>
      <Phone aria-hidden className="mt-0.5 h-3.5 w-3.5 shrink-0" />
      <p>{t('read.callback', { numbers: numbers.join(', ') })}</p>
    </div>
  )
}
