import type { LucideIcon } from 'lucide-react'

import { useAtomValue, useSetAtom } from 'jotai'
import {
  AlarmClock,
  Archive,
  Bell,
  FileText,
  Hourglass,
  Inbox,
  MailOpen,
  Mails,
  PanelLeftClose,
  Pin,
  Send,
  ShieldAlert,
  ShieldQuestion,
  SquarePen,
  Star,
  Trash2,
  UserCheck,
} from 'lucide-react'

import { LabelDot } from '@/components/label-chip'
import { useUnreadCounts } from '@/hooks/use-current-mail-filters'
import { useLabels } from '@/hooks/use-labels'
import { useOpenList } from '@/hooks/use-open-list'
import { useT } from '@/i18n'
import { countFor } from '@/lib/folder-counts'
import { MAIL_LIST_TABS, MAIL_LISTS, type MailListId } from '@/lib/mail-lists'
import { rovingKeyDown } from '@/lib/roving'
import { openNewMessageAtom } from '@/store/composer-windows'
import { foldersOpenAtom } from '@/store/folders-column'
import { activeLabelAtom, activeListAtom, selectLabelAtom } from '@/store/ui'

const ICONS: Record<MailListId, LucideIcon> = {
  all: Mails,
  archived: Archive,
  assigned: UserCheck,
  awaiting: Hourglass,
  draft: FileText,
  inbox: Inbox,
  junk: ShieldAlert,
  label: Inbox,
  np: Bell,
  pinned: Pin,
  quarantine: ShieldQuestion,
  send: Send,
  snoozed: AlarmClock,
  starred: Star,
  trash: Trash2,
  unread: MailOpen,
}

/**
 * The mail lists as a column of folders, with the user's labels under
 * them: the wide-screen form of the tab chips, which narrower screens keep.
 * A title row with the button that collapses the column, and Compose,
 * sit above the folders.
 */
export function MailFolders() {
  const t = useT()
  const active = useAtomValue(activeListAtom)
  const activeLabel = useAtomValue(activeLabelAtom)
  const open = useOpenList()
  const selectLabel = useSetAtom(selectLabelAtom)
  const unread = useUnreadCounts()
  const { data: labels = [] } = useLabels()
  const setFoldersOpen = useSetAtom(foldersOpenAtom)
  const openNewMessage = useSetAtom(openNewMessageAtom)

  return (
    <nav className="flex h-full flex-col overflow-y-auto px-2 pb-2 select-none">
      <div className="flex h-[3.25rem] shrink-0 items-center justify-between pr-0.5 pl-2.5">
        <h2 className="text-fg text-base font-semibold">{t('nav.mail')}</h2>
        <button
          aria-label={t('folders.collapse')}
          className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary focus-visible:ring-accent/50 flex h-8 w-8 items-center justify-center rounded-md transition-colors focus-visible:ring-2 focus-visible:outline-none"
          onClick={() => setFoldersOpen(false)}
          title={t('folders.collapse')}
          type="button"
        >
          <PanelLeftClose aria-hidden className="h-4 w-4" />
        </button>
      </div>
      <button
        className={`${folderClass(false)} mb-3`}
        onClick={() => openNewMessage()}
        type="button"
      >
        <SquarePen aria-hidden className="h-4 w-4 shrink-0" />
        <span className="min-w-0 flex-1 text-left">{t('list.newConversation')}</span>
      </button>
      <div
        aria-label={t('list.tabs')}
        aria-orientation="vertical"
        className="flex flex-col gap-0.5"
        onKeyDown={(e) => {
          rovingKeyDown(e, '[role="tab"]', 'vertical')
          rovingKeyDown(e, '[role="tab"]', 'horizontal')
        }}
        role="tablist"
      >
        {MAIL_LIST_TABS.map((id) => (
          <FolderButton
            count={countFor(id, unread)}
            icon={ICONS[id]}
            key={id}
            label={t(MAIL_LISTS[id].label)}
            onClick={() => open(id)}
            selected={active === id}
          />
        ))}
      </div>
      {labels.length > 0 && (
        <div aria-label={t('labels.title')} className="mt-4 flex flex-col gap-0.5" role="group">
          <h3 className="text-fg-muted px-2.5 pb-1 text-xs">{t('labels.title')}</h3>
          {labels.map((l) => (
            <button
              aria-pressed={active === 'label' && activeLabel === l.id}
              className={folderClass(active === 'label' && activeLabel === l.id)}
              key={l.id}
              onClick={() => selectLabel(l.id)}
              type="button"
            >
              <LabelDot color={l.color} />
              <span className="min-w-0 flex-1 truncate text-left">{l.name}</span>
            </button>
          ))}
        </div>
      )}
    </nav>
  )
}

function FolderButton({
  count,
  icon: Icon,
  label,
  onClick,
  selected,
}: {
  count: number
  icon: LucideIcon
  label: string
  onClick: () => void
  selected: boolean
}) {
  return (
    <button
      aria-selected={selected}
      className={folderClass(selected)}
      onClick={onClick}
      role="tab"
      tabIndex={selected ? 0 : -1}
      type="button"
    >
      <Icon aria-hidden className="h-4 w-4 shrink-0" />
      {/* wraps rather than cuts, between phrases in Japanese */}
      <span className="break-phrase min-w-0 flex-1 text-left leading-snug break-words">
        {label}
      </span>
      {count > 0 && <span className="text-fg-muted text-xs tabular-nums">{count}</span>}
    </button>
  )
}

function folderClass(selected: boolean): string {
  const base =
    'focus-visible:ring-accent/50 flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm transition-colors focus-visible:ring-2 focus-visible:outline-none'
  if (selected) return `${base} bg-accent/10 text-fg font-medium`
  return `${base} text-fg-secondary hover:bg-bg-secondary`
}
