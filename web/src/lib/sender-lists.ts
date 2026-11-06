import { queryClient } from '@/lib/query-client'
import { type SenderListName, wireListSenders } from '@/wire/endpoints/settings'

/**
 * The reader's two sender lists, as Settings → Senders and the message
 * menu's Block item both read them: one cache, so blocking from a message
 * shows in Settings and the reverse.
 */
export const sendersKey = (list: SenderListName) => ['settings', 'senders', list] as const

/**
 * Both lists: the server takes an address off one list when it goes on
 * the other, so a write to either changes both.
 */
export function invalidateSenderLists(): void {
  void queryClient.invalidateQueries({ queryKey: ['settings', 'senders'] })
}

export function sendersQuery(list: SenderListName, enabled = true) {
  return {
    enabled,
    queryKey: sendersKey(list),
    queryFn: () => wireListSenders(list).then((rows) => [...rows].sort()),
  }
}
