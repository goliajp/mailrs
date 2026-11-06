import type { Key } from '@/i18n'

import { toast } from '@goliapkg/gds'
import { useAtom, useAtomValue } from 'jotai'
import { useState } from 'react'

import { useConversationRows } from '@/hooks/use-current-list'
import { useOpenList } from '@/hooks/use-open-list'
import { useSectionedInbox } from '@/hooks/use-sectioned-inbox'
import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { latestSenders } from '@/lib/latest-senders'
import { invalidateMail } from '@/lib/mail-cache'
import { rovingKeyDown } from '@/lib/roving'
import { authAtom } from '@/store/auth'
import { INBOX_SECTIONS, type InboxSection, inboxSectionAtom } from '@/store/inbox-sections'
import { activeListAtom } from '@/store/ui'
import { wireAcceptSenders } from '@/wire/endpoints/senders'

const LABELS: Record<InboxSection, Key> = {
  followup: 'sections.followup',
  new: 'sections.new',
  other: 'sections.other',
  reply: 'sections.reply',
}

/**
 * The sectioned inbox's sections, under the tabs while the Inbox is shown
 * by section. Notifications and promotions are their own tab already, so
 * they are one more entry here that opens it.
 */
export function InboxSectionBar() {
  const t = useT()
  const activeList = useAtomValue(activeListAtom)
  const sectioned = useSectionedInbox()
  const [section, setSection] = useAtom(inboxSectionAtom)
  const openList = useOpenList()
  if (activeList !== 'inbox' || !sectioned) return null
  return (
    <div className="border-border flex items-center gap-1 border-b px-3 py-1.5">
      <div
        aria-label={t('sections.bar')}
        className="flex flex-1 flex-wrap gap-1"
        onKeyDown={(e) => rovingKeyDown(e, '[role="tab"]', 'horizontal')}
        role="tablist"
      >
        {INBOX_SECTIONS.map((s) => (
          <button
            aria-selected={section === s}
            className={chipClass(section === s)}
            key={s}
            onClick={() => setSection(s)}
            role="tab"
            type="button"
          >
            {t(LABELS[s])}
          </button>
        ))}
        <button className={chipClass(false)} onClick={() => openList('np')} type="button">
          {t('sections.feed')} →
        </button>
      </div>
      {section === 'new' && <AcceptAll />}
    </div>
  )
}

/** Accept the senders of the conversations shown in New. */
function AcceptAll() {
  const t = useT()
  const me = (useAtomValue(authAtom)?.address ?? '').toLowerCase()
  const { rows } = useConversationRows()
  const [busy, setBusy] = useState(false)
  const senders = latestSenders(rows, me)
  if (senders.length === 0) return null
  const accept = async () => {
    setBusy(true)
    try {
      const r = await wireAcceptSenders(senders)
      toast.success(t('sections.accepted', { count: r.accepted }))
      invalidateMail()
    } catch (e) {
      toast.error(errorText(e, t('sections.acceptFailed')))
    } finally {
      setBusy(false)
    }
  }
  return (
    <button
      className="text-accent shrink-0 text-xs hover:underline disabled:opacity-50"
      disabled={busy}
      onClick={() => void accept()}
      type="button"
    >
      {t('sections.accept')}
    </button>
  )
}

function chipClass(active: boolean): string {
  const base = 'cursor-pointer rounded-md px-2 py-1 text-xs transition-colors'
  if (active) return `${base} bg-border-strong text-fg font-semibold`
  return `${base} bg-bg-secondary text-fg-muted hover:bg-bg-tertiary hover:text-fg-secondary`
}
