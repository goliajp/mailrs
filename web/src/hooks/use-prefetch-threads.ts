/**
 * Fetch conversations before they are opened.
 *
 * Opening a conversation nobody has opened yet is one round trip, and on
 * a phone network that round trip is most of the wait: measured on a
 * throttled Fast 4G link, about 80% of the time between pressing `j`
 * and the next conversation appearing. The conversations most likely to
 * be opened next are the ones beside the current one, in the direction
 * people read (down), and the one under the pointer.
 *
 * Both fill the same cache entry the reading pane reads
 * (`threadQueryOptions`), so an open that finds it there draws at once,
 * and one that finds the fetch still in flight joins it instead of
 * starting another.
 */

import type { SelectableRow } from '@/lib/list-selection'

import { useQueryClient } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'
import { useCallback, useEffect, useRef } from 'react'

import { threadQueryOptions } from '@/hooks/use-mail-queries'
import { selectedDomainsAtom } from '@/store/ui'

/** Rows after the current one to fetch ahead, and rows before it. */
export const AHEAD = 3
export const BEHIND = 1

/** How long the pointer rests on a row before it counts as intent. */
export const HOVER_INTENT_MS = 60

/**
 * The conversations to fetch around `current`: the next `AHEAD` rows
 * nearest first, then the `BEHIND` rows above. Nothing when the current
 * conversation is not in the list.
 */
export function threadsToPrefetch(
  rows: readonly SelectableRow[],
  current: null | string
): string[] {
  if (!current) return []
  const at = rows.findIndex((r) => r.threadId === current)
  if (at < 0) return []
  const out: string[] = []
  for (let i = at + 1; i <= at + AHEAD && i < rows.length; i++) out.push(rows[i].threadId)
  for (let i = at - 1; i >= at - BEHIND && i >= 0; i--) out.push(rows[i].threadId)
  return out
}

/**
 * `enter` / `leave` for a row: a pointer that stays `HOVER_INTENT_MS` on
 * it fetches that conversation. Passing over rows on the way somewhere
 * else fetches nothing.
 */
export function useHoverPrefetch(): { enter: (threadId: string) => void; leave: () => void } {
  const qc = useQueryClient()
  const domains = useAtomValue(selectedDomainsAtom)
  const timer = useRef<null | ReturnType<typeof setTimeout>>(null)

  const leave = useCallback(() => {
    if (timer.current !== null) clearTimeout(timer.current)
    timer.current = null
  }, [])
  const enter = useCallback(
    (threadId: string) => {
      leave()
      timer.current = setTimeout(() => {
        timer.current = null
        void qc.prefetchQuery(threadQueryOptions(threadId, domains))
      }, HOVER_INTENT_MS)
    },
    [qc, domains, leave]
  )
  useEffect(() => leave, [leave])
  return { enter, leave }
}

/**
 * Fetch the neighbours of the conversation on screen once its own
 * messages are in (`ready`), so they never compete with it for the link.
 */
export function usePrefetchNeighbours(
  rows: readonly SelectableRow[],
  current: null | string,
  ready: boolean
): void {
  const qc = useQueryClient()
  const domains = useAtomValue(selectedDomainsAtom)
  const ids = threadsToPrefetch(rows, current).join('\n')
  useEffect(() => {
    if (!ready || ids === '') return
    for (const id of ids.split('\n')) {
      void qc.prefetchQuery(threadQueryOptions(id, domains))
    }
  }, [qc, domains, ids, ready])
}
