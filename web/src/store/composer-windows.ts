import type { ComposerStart } from '@/components/composer/start'

import { atom } from 'jotai'

import { isMobileScreen } from '@/hooks/use-is-mobile'

import { composeReplySourceAtom, composingNewAtom } from './ui'

/** One floating composer: its own compose session, open or folded to its title bar. */
export type ComposerWindow = { id: string; minimized: boolean; start: ComposerStart }

/** More open than this and the oldest open one folds down. */
export const MAX_OPEN_WINDOWS = 3

export const composerWindowsAtom = atom<ComposerWindow[]>([])

let lastId = 0

/**
 * A new message: a floating window on a desktop, the page on a phone,
 * where there is no room beside anything for a window.
 */
export const openNewMessageAtom = atom(null, (get, set) => {
  if (isMobileScreen()) {
    set(composeReplySourceAtom, null)
    set(composingNewAtom, true)
    return
  }
  lastId += 1
  const win: ComposerWindow = { id: `w${lastId}`, minimized: false, start: { kind: 'new' } }
  set(composerWindowsAtom, withOpen([...get(composerWindowsAtom), win], win.id))
})

export const closeComposerWindowAtom = atom(null, (get, set, id: string) => {
  set(
    composerWindowsAtom,
    get(composerWindowsAtom).filter((w) => w.id !== id)
  )
})

export const toggleComposerWindowAtom = atom(null, (get, set, id: string) => {
  const list = get(composerWindowsAtom)
  const target = list.find((w) => w.id === id)
  if (!target) return
  if (target.minimized) {
    set(composerWindowsAtom, withOpen(list, id))
    return
  }
  set(composerWindowsAtom, patched(list, id, { minimized: true }))
})

/**
 * `id` open, and the oldest other open windows folded until no more than
 * `MAX_OPEN_WINDOWS` are open.
 */
export function withOpen(list: ComposerWindow[], id: string): ComposerWindow[] {
  let next = patched(list, id, { minimized: false })
  let open = next.filter((w) => !w.minimized).length
  for (const w of list) {
    if (open <= MAX_OPEN_WINDOWS) break
    if (w.id === id || w.minimized) continue
    next = patched(next, w.id, { minimized: true })
    open -= 1
  }
  return next
}

function patched(
  list: ComposerWindow[],
  id: string,
  patch: Partial<ComposerWindow>
): ComposerWindow[] {
  return list.map((w) => {
    if (w.id !== id) return w
    return { ...w, ...patch }
  })
}
