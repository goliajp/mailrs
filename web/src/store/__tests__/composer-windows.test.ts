import { createStore } from 'jotai'
import { describe, expect, it } from 'vitest'

import {
  composerWindowsAtom,
  MAX_OPEN_WINDOWS,
  openNewMessageAtom,
  toggleComposerWindowAtom,
} from '../composer-windows'

function openIds(store: ReturnType<typeof createStore>): string[] {
  return store
    .get(composerWindowsAtom)
    .filter((w) => !w.minimized)
    .map((w) => w.id)
}

describe('floating composer windows', () => {
  it('folds the oldest open window when one more opens than fit', () => {
    const store = createStore()
    for (let i = 0; i < MAX_OPEN_WINDOWS + 1; i++) store.set(openNewMessageAtom)
    const all = store.get(composerWindowsAtom)
    expect(all).toHaveLength(MAX_OPEN_WINDOWS + 1)
    expect(all[0].minimized).toBe(true)
    expect(openIds(store)).toEqual(all.slice(1).map((w) => w.id))
  })

  it('restoring a folded window folds the oldest other one', () => {
    const store = createStore()
    for (let i = 0; i < MAX_OPEN_WINDOWS + 1; i++) store.set(openNewMessageAtom)
    const [first, second] = store.get(composerWindowsAtom)
    store.set(toggleComposerWindowAtom, first.id)
    expect(openIds(store)).toContain(first.id)
    expect(openIds(store)).not.toContain(second.id)
    expect(openIds(store)).toHaveLength(MAX_OPEN_WINDOWS)
  })
})
