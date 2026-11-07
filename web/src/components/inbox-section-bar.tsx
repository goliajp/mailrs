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
import { rovingKeyDown, tabStop } from '@/lib/roving'
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
 * The sectioned inbox's sections on a row of their own under the tabs,
 * below xl. From xl the tabs are hidden and the same chips sit in the
 * filter row instead (`FilterBar`), so this row is not shown there.
 */
export function InboxSectionBar() {
  return (
    <InboxSectionChips className="border-border flex items-center gap-1 border-b px-3 py-1.5 xl:hidden" />
  )
}

/**
 * The four sections plus Notifications and promotions, which is a list of
 * its own rather than a section. Shown in the Inbox and in that list while
 * the Inbox is shown by section. In the Inbox the chosen section is the
 * selected chip and the last chip leads on to the list (→); in the list
 * that chip is selected and each section leads back to the Inbox (←).
 */
export function InboxSectionChips({ className }: { className: string }) {
  const t = useT()
  const activeList = useAtomValue(activeListAtom)
  const sectioned = useSectionedInbox()
  const [section, setSection] = useAtom(inboxSectionAtom)
  const openList = useOpenList()
  if (!sectioned) return null
  if (activeList !== 'inbox' && activeList !== 'np') return null
  const inInbox = activeList === 'inbox'
  // in the Inbox the chosen chip clears the section and shows the whole inbox
  const pick = (s: InboxSection) => {
    if (inInbox && section === s) {
      setSection(null)
      return
    }
    setSection(s)
    openList('inbox')
  }
  // with nothing selected the first chip is the Tab stop
  const noneSelected = inInbox && section === null
  return (
    <div className={className}>
      <div
        aria-label={t('sections.bar')}
        className="flex flex-1 flex-wrap gap-1"
        onKeyDown={(e) => rovingKeyDown(e, '[role="tab"]', 'horizontal')}
        role="tablist"
      >
        {INBOX_SECTIONS.map((s, i) => {
          const selected = inInbox && section === s
          return (
            <button
              aria-selected={selected}
              className={chipClass(selected)}
              key={s}
              onClick={() => pick(s)}
              role="tab"
              tabIndex={tabStop(selected || (noneSelected && i === 0))}
              type="button"
            >
              {sectionText(t(LABELS[s]), inInbox)}
            </button>
          )
        })}
        <button
          aria-selected={!inInbox}
          className={chipClass(!inInbox)}
          onClick={() => openList('np')}
          role="tab"
          tabIndex={tabStop(!inInbox)}
          type="button"
        >
          {feedText(t('sections.feed'), inInbox)}
        </button>
      </div>
      {inInbox && section === 'new' && <AcceptAll />}
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

function feedText(label: string, inInbox: boolean): string {
  if (inInbox) return `${label} →`
  return label
}

function sectionText(label: string, inInbox: boolean): string {
  if (inInbox) return label
  return `← ${label}`
}
