import { useQuery } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'

import { useOpenList } from '@/hooks/use-open-list'
import { useT } from '@/i18n'
import { activeListAtom } from '@/store/ui'
import { fetchRecalled } from '@/wire/endpoints/fraud'

const EMPTY: never[] = []

/**
 * Above the Inbox, where they used to be: conversations the reader had
 * that were moved to Review when new information about their sender made
 * the rules hold them.
 */
export function RecalledNote() {
  const t = useT()
  const activeList = useAtomValue(activeListAtom)
  const openList = useOpenList()
  const { data } = useQuery({
    enabled: activeList === 'inbox',
    queryKey: ['review', 'recalled'],
    // written by the core when a re-judging round holds a conversation;
    // a minute late is fine for a line that stays for a week
    staleTime: 60_000,
    queryFn: ({ signal }) => fetchRecalled(signal),
  })
  const items = data ?? EMPTY
  if (activeList !== 'inbox' || items.length === 0) return null
  return (
    <p className="border-border bg-bg-secondary text-fg-secondary border-b px-3 py-2 text-xs">
      {t('list.recalled', { count: items.length })}{' '}
      <button
        className="text-accent hover:underline"
        onClick={() => openList('quarantine')}
        type="button"
      >
        {t('list.recalledOpen')}
      </button>
    </p>
  )
}
