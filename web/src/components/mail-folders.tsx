import type { LucideIcon } from 'lucide-react'

import { useAtomValue, useSetAtom } from 'jotai'
import {
  Archive,
  Bell,
  FileText,
  Inbox,
  MailOpen,
  Send,
  ShieldAlert,
  ShieldQuestion,
  Star,
  Trash2,
} from 'lucide-react'

import { LabelDot } from '@/components/label-chip'
import { useCurrentUnreadCount } from '@/hooks/use-current-mail-filters'
import { useLabels } from '@/hooks/use-labels'
import { useOpenList } from '@/hooks/use-open-list'
import { useT } from '@/i18n'
import { MAIL_LIST_TABS, MAIL_LISTS, type MailListId } from '@/lib/mail-lists'
import { rovingKeyDown } from '@/lib/roving'
import { activeLabelAtom, activeListAtom, selectLabelAtom } from '@/store/ui'

const ICONS: Record<(typeof MAIL_LIST_TABS)[number], LucideIcon> = {
  archived: Archive,
  draft: FileText,
  inbox: Inbox,
  junk: ShieldAlert,
  label: Inbox,
  np: Bell,
  quarantine: ShieldQuestion,
  send: Send,
  starred: Star,
  trash: Trash2,
  unread: MailOpen,
}

/**
 * The mail lists as a column of folders, with the user's labels under
 * them: the wide-screen form of the tab chips, which narrower screens keep.
 */
export function MailFolders() {
  const t = useT()
  const active = useAtomValue(activeListAtom)
  const activeLabel = useAtomValue(activeLabelAtom)
  const open = useOpenList()
  const selectLabel = useSetAtom(selectLabelAtom)
  const unread = useCurrentUnreadCount()
  const { data: labels = [] } = useLabels()

  return (
    <nav className="flex h-full flex-col overflow-y-auto p-2 select-none">
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
          <p className="text-fg-muted px-2.5 pb-1 text-xs">{t('labels.title')}</p>
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

function countFor(id: MailListId, unread: number): number {
  if (id === 'inbox') return unread
  return 0
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
      <span className="min-w-0 flex-1 truncate text-left">{label}</span>
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
