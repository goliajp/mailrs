import { useAtom, useAtomValue } from 'jotai'

import { formatDate } from '@/lib/format'
import { FLAG_SEEN } from '@/lib/types'
import { messagesAtom, selectedMessageUidAtom } from '@/store/mail'

export function MessageList() {
  const messages = useAtomValue(messagesAtom)
  const [selectedUid, setSelectedUid] = useAtom(selectedMessageUidAtom)

  if (messages.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-zinc-400">
        No messages
      </div>
    )
  }

  return (
    <div className="flex flex-col overflow-y-auto">
      {messages.map((msg) => {
        const read = (msg.flags & FLAG_SEEN) !== 0
        return (
          <button
            key={msg.uid}
            onClick={() => setSelectedUid(msg.uid)}
            className={`flex flex-col gap-0.5 border-b border-zinc-100 px-4 py-3 text-left transition-colors dark:border-zinc-800/50 ${
              selectedUid === msg.uid
                ? 'bg-zinc-100 dark:bg-zinc-800/60'
                : 'hover:bg-zinc-50 dark:hover:bg-zinc-900/40'
            }`}
          >
            <div className="flex items-center justify-between gap-2">
              <span
                className={`truncate text-sm ${!read ? 'font-semibold text-zinc-900 dark:text-zinc-100' : 'text-zinc-600 dark:text-zinc-400'}`}
              >
                {msg.sender || '(unknown)'}
              </span>
              <span className="shrink-0 text-xs text-zinc-400 tabular-nums">
                {formatDate(msg.internal_date)}
              </span>
            </div>
            <div
              className={`truncate text-sm ${!read ? 'font-medium text-zinc-800 dark:text-zinc-200' : 'text-zinc-500 dark:text-zinc-500'}`}
            >
              {msg.subject || '(no subject)'}
            </div>
          </button>
        )
      })}
    </div>
  )
}
