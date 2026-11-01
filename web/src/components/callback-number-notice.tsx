import type { ThreadMessage } from '@/lib/types'

import { Phone } from 'lucide-react'
import { useMemo } from 'react'

import { useT } from '@/i18n'
import { newCallbackNumbers } from '@/lib/callback-numbers'

/**
 * The phone numbers a message gives that its sender has not written
 * before in this conversation. A notice, not a verdict: a number proves
 * nothing about who answers it, and invoices and receipts carry them too.
 */
export function CallbackNumberNotice({
  conversation,
  message,
}: {
  conversation: readonly ThreadMessage[]
  message: ThreadMessage
}) {
  const t = useT()
  const numbers = useMemo(() => {
    const earlier = conversation.slice(0, conversation.indexOf(message))
    return newCallbackNumbers(message, earlier)
  }, [message, conversation])
  if (numbers.length === 0) return null
  return (
    <div className="text-fg-muted flex items-start gap-2 px-4 py-2 text-xs" data-callback-notice>
      <Phone aria-hidden className="mt-0.5 h-3.5 w-3.5 shrink-0" />
      <p>{t('read.callback', { numbers: numbers.join(', ') })}</p>
    </div>
  )
}
