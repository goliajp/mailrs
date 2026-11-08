import type { Key } from '@/i18n'

import { toast } from '@goliapkg/gds'
import { useAtom, useAtomValue } from 'jotai'
import { useState } from 'react'

import { useConversationRows } from '@/hooks/use-current-list'
import { useOpenList } from '@/hooks/use-open-list'
import { type SectionCount, useSectionCount } from '@/hooks/use-section-count'
import { useSectionedInbox } from '@/hooks/use-sectioned-inbox'
import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { latestSenders } from '@/lib/latest-senders'
import { invalidateMail } from '@/lib/mail-cache'
import { rovingKeyDown, tabStop } from '@/lib/roving'
import { authAtom } from '@/store/auth'
import { type InboxSection, inboxSectionAtom } from '@/store/inbox-sections'
import { activeListAtom } from '@/store/ui'
import { wireAcceptSenders } from '@/wire/endpoints/senders'

/**
 * The sections offered as chips. Other is not one: its conversations are
 * the rest of the whole inbox, which is what no chip shows.
 */
type ChipSection = Exclude<InboxSection, 'other'>

const CHIP_SECTIONS: readonly ChipSection[] = ['reply', 'followup', 'new']

const LABELS: Record<ChipSection, Key> = {
  followup: 'sections.followup',
  new: 'sections.new',
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
 * The inbox's sections as optional filters, plus Notifications and
 * promotions, which is a list of its own. Shown in the Inbox and in that
 * list while the Inbox is shown by section. The inbox opens whole with no
 * chip chosen; a chosen chip narrows it and a second click on it widens it
 * again. Reply is always there; Follow up and New only while they hold
 * something, with how many, and a chosen one stays until the reader leaves
 * it so it does not vanish from under them. In the list its own chip is
 * the chosen one and each section leads back to the Inbox (←).
 */
export function InboxSectionChips({ className }: { className: string }) {
  const t = useT()
  const activeList = useAtomValue(activeListAtom)
  const sectioned = useSectionedInbox()
  const [section, setSection] = useAtom(inboxSectionAtom)
  const openList = useOpenList()
  const inInbox = activeList === 'inbox'
  const shown = sectioned && (inInbox || activeList === 'np')
  const counts: Record<ChipSection, null | SectionCount> = {
    followup: useSectionCount('followup', shown),
    new: useSectionCount('new', shown),
    reply: null,
  }
  if (!shown) return null
  const chosen = (s: ChipSection) => inInbox && section === s
  const chips = CHIP_SECTIONS.filter((s) => s === 'reply' || (counts[s]?.n ?? 0) > 0 || chosen(s))
  const pick = (s: ChipSection) => {
    if (chosen(s)) {
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
        {chips.map((s, i) => (
          <button
            aria-selected={chosen(s)}
            className={chipClass(chosen(s))}
            key={s}
            onClick={() => pick(s)}
            role="tab"
            tabIndex={tabStop(chosen(s) || (noneSelected && i === 0))}
            type="button"
          >
            {sectionText(withCount(t(LABELS[s]), counts[s]), inInbox)}
          </button>
        ))}
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

/** `label` with how many, or the label alone when there is no count or it is 0. */
function withCount(label: string, count: null | SectionCount): string {
  if (!count || count.n === 0) return label
  if (count.full) return `${label} ${count.n}+`
  return `${label} ${count.n}`
}
