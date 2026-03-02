import { useAtom, useAtomValue, useSetAtom } from 'jotai'

import { avatarColor, avatarInitial, extractName } from '@/lib/avatar'
import { formatDate } from '@/lib/format'
import type { ConversationSummary } from '@/lib/types'
import {
  composingNewAtom,
  conversationsAtom,
  searchQueryAtom,
  selectedThreadIdAtom,
} from '@/store/chat'

function ConversationItem({
  convo,
  selected,
  onSelect,
}: {
  convo: ConversationSummary
  selected: boolean
  onSelect: () => void
}) {
  const firstParticipant = convo.participants[0] ?? ''
  const name = extractName(firstParticipant)
  const initial = avatarInitial(firstParticipant)
  const color = avatarColor(firstParticipant)
  const hasUnread = convo.unread_count > 0

  return (
    <button
      onClick={onSelect}
      className={`flex w-full items-start gap-3 px-4 py-3 text-left transition-colors ${
        selected
          ? 'bg-zinc-100 dark:bg-zinc-800'
          : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50'
      }`}
    >
      <div
        className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-sm font-medium text-white ${color}`}
      >
        {initial}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-2">
          <span
            className={`truncate text-sm ${hasUnread ? 'font-semibold text-zinc-900 dark:text-zinc-100' : 'text-zinc-700 dark:text-zinc-300'}`}
          >
            {name}
            {convo.participants.length > 1 && (
              <span className="text-zinc-400">
                {' '}
                +{convo.participants.length - 1}
              </span>
            )}
          </span>
          <span className="shrink-0 text-xs text-zinc-400">
            {formatDate(convo.last_date)}
          </span>
        </div>
        <div className="flex items-center justify-between gap-2">
          <p
            className={`truncate text-sm ${hasUnread ? 'font-medium text-zinc-800 dark:text-zinc-200' : 'text-zinc-500 dark:text-zinc-400'}`}
          >
            {convo.subject || '(no subject)'}
          </p>
          {hasUnread && (
            <span className="flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-blue-500 px-1.5 text-xs font-medium text-white">
              {convo.unread_count}
            </span>
          )}
        </div>
      </div>
    </button>
  )
}

export function ConversationList() {
  const conversations = useAtomValue(conversationsAtom)
  const [selectedId, setSelectedId] = useAtom(selectedThreadIdAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const [searchQuery, setSearchQuery] = useAtom(searchQueryAtom)

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-zinc-200 p-3 dark:border-zinc-800">
        <div className="relative flex-1">
          <svg
            className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-400"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
          >
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.3-4.3" />
          </svg>
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search..."
            className="w-full rounded-md border border-zinc-200 bg-zinc-50 py-1.5 pl-9 pr-3 text-sm text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-400 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:border-zinc-500"
          />
        </div>
        <button
          onClick={() => {
            setComposingNew(true)
            setSelectedId(null)
          }}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-zinc-500 transition-colors hover:bg-zinc-100 dark:hover:bg-zinc-800"
          title="New conversation"
        >
          <svg
            className="h-5 w-5"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M16.862 4.487l1.687-1.688a1.875 1.875 0 112.652 2.652L6.832 19.82a4.5 4.5 0 01-1.897 1.13l-2.685.8.8-2.685a4.5 4.5 0 011.13-1.897L16.863 4.487zm0 0L19.5 7.125"
            />
          </svg>
        </button>
      </div>

      <div className="flex-1 overflow-y-auto">
        {conversations.length === 0 ? (
          <div className="p-6 text-center text-sm text-zinc-400">
            No conversations
          </div>
        ) : (
          conversations.map((c) => (
            <ConversationItem
              key={c.thread_id}
              convo={c}
              selected={selectedId === c.thread_id}
              onSelect={() => {
                setSelectedId(c.thread_id)
                setComposingNew(false)
              }}
            />
          ))
        )}
      </div>
    </div>
  )
}
