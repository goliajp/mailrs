import { getDefaultStore } from 'jotai'
import { useEffect, useRef } from 'react'
import { useLocation } from 'react-router'

import { isStaleBuild, loadedEntryScript } from '@/lib/build-entry'
import { http } from '@/lib/http'
import { composerWindowsAtom } from '@/store/composer-windows'
import {
  composeDraftSourceAtom,
  composeRedraftSourceAtom,
  composeReplySourceAtom,
  composingNewAtom,
} from '@/store/ui'

const CHECK_EVERY_MS = 5 * 60 * 1000

/** A text area, or an editable region, that is not empty. */
export function hasUnsentText(doc: Document): boolean {
  for (const el of doc.querySelectorAll('textarea')) {
    if (el.value.trim() !== '') return true
  }
  for (const el of doc.querySelectorAll('[contenteditable="true"]')) {
    if ((el.textContent ?? '').trim() !== '') return true
  }
  return false
}

/**
 * Load the deployed build once one exists, at a moment the reader is
 * not in the middle of something: on coming back to the tab, or on the
 * next navigation, and never while a message is being written.
 *
 * A tab left open kept the code it loaded with: on 2026-11-02 a reader
 * was shown a verdict panel two releases old, which drew the classifier's
 * filing note as "something looked wrong", while the server and the
 * deployed page both said otherwise.
 */
export function useReloadOnNewBuild(): void {
  const stale = useRef(false)
  const { pathname } = useLocation()

  useEffect(() => {
    const loaded = loadedEntryScript(document)
    if (loaded === null) return
    let cancelled = false
    const check = async () => {
      try {
        const res = await http('/index.html', { cache: 'no-store' })
        if (!res.ok || cancelled) return
        stale.current = isStaleBuild(loaded, await res.text())
      } catch {
        // offline: ask again at the next check
      }
    }
    const onVisible = () => {
      if (document.visibilityState !== 'visible') return
      if (stale.current && !writing()) {
        window.location.reload()
        return
      }
      void check()
    }
    void check()
    const id = setInterval(() => void check(), CHECK_EVERY_MS)
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      cancelled = true
      clearInterval(id)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [])

  useEffect(() => {
    if (stale.current && !writing()) window.location.reload()
  }, [pathname])
}

/**
 * Whether a composer is open, or any field on the page holds text. The
 * inline reply box keeps its text in its own state, so the page is asked
 * as well as the store.
 */
function writing(): boolean {
  const store = getDefaultStore()
  if (
    store.get(composingNewAtom) ||
    store.get(composerWindowsAtom).length > 0 ||
    store.get(composeReplySourceAtom) !== null ||
    store.get(composeDraftSourceAtom) !== null ||
    store.get(composeRedraftSourceAtom) !== null
  ) {
    return true
  }
  return hasUnsentText(document)
}
