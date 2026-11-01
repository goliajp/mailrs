import { toast } from '@goliapkg/gds'
import { useState } from 'react'

import { type Key, useT } from '@/i18n'
import { type UnsubscribeHeader, unsubscribeOffer } from '@/lib/unsubscribe-offer'
import { wireUnsubscribe } from '@/wire/endpoints/mail'

export function UnsubscribeFooter({
  header,
  threadId,
  uid,
}: {
  header: undefined | UnsubscribeHeader
  threadId: string
  uid: number
}) {
  const t = useT()
  const [state, setState] = useState<'done' | 'failed' | 'idle' | 'working'>('idle')
  const offer = unsubscribeOffer(header)

  if (offer.kind === 'none') return null

  const link = 'text-fg-muted hover:text-fg text-xs underline'

  if (state === 'done') {
    return <p className="text-fg-muted mt-2 text-xs">{t('unsubscribe.done')}</p>
  }

  if (offer.kind === 'one-click') {
    const run = async () => {
      setState('working')
      try {
        const result = await wireUnsubscribe(threadId, uid)
        // `ok: false` is a 200 — the request reached the sender and
        // they refused. Saying "done" there is how people end up
        // clicking this every week for a year.
        if (!result.ok) {
          setState('failed')
          toast.error(result.message || t('unsubscribe.refused'))
          return
        }
        setState('done')
      } catch {
        setState('failed')
        toast.error(t('unsubscribe.unreachable'))
      }
    }
    return (
      <div className="mt-2">
        <button className={link} disabled={state === 'working'} onClick={() => void run()}>
          {t(workingLabel(state === 'working'))}
        </button>
        {state === 'failed' && (
          <span className="text-fg-muted ml-2 text-xs">{t('unsubscribe.failedNote')}</span>
        )}
      </div>
    )
  }

  return (
    <div className="mt-2">
      <a className={link} href={offer.url} rel="noreferrer noopener" target="_blank">
        {t(offerLabel(offer.kind === 'page'))}
      </a>
    </div>
  )
}

function offerLabel(page: boolean): Key {
  if (page) return 'unsubscribe.page'
  return 'unsubscribe.email'
}

/**
 * The way off a mailing list, under the message that came from one.
 *
 * At the foot of the message rather than a banner over it: 42.6% of
 * real mail carries `List-Unsubscribe`, so a banner would be a stripe
 * over nearly every other message, and the reader who wants out has
 * already finished reading.
 *
 * The same three answers iOS offers, from the same rule — see
 * `lib/unsubscribe-offer.ts`. Only one-click is performed on the
 * reader's behalf; a page is a link, because loading it hands the
 * sender their IP.
 */
function workingLabel(working: boolean): Key {
  if (working) return 'unsubscribe.working'
  return 'unsubscribe.action'
}
