import type { MailListId } from '@/lib/mail-lists'

import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useEffect } from 'react'

import { useConversationActions } from '@/hooks/use-conversation-actions'
import {
  useConversationRows,
  useCurrentListRows,
  useSelectedThreadId,
  useSelectThreadId,
} from '@/hooks/use-current-list'
import { actionForKey, ACTIONS, keyName, rowStateOf, viewOf } from '@/lib/actions'
import { isEditableTarget } from '@/lib/editable-target'
import {
  batchModeAtom,
  categoryFilterAtom,
  composeReplySourceAtom,
  composingNewAtom,
  folderAtom,
  importanceSectionAtom,
  lastUndoAtom,
  mobileViewAtom,
  selectedThreadIdsAtom,
  selectMailListAtom,
  threadShortcutAtom,
} from '@/store/ui'

/// The list a `g` chord goes to, or nothing when the second key is not
/// part of one.
///
/// Exported so the shortcuts sheet reads the same table the handler
/// does: it advertised `g a`, which was never implemented, and `g s`,
/// which the switch below it shadowed. A help panel that lies is worse
/// than none.
export function chordList(key: string): MailListId | null {
  switch (key) {
    case 'a':
      return 'archived'
    case 'd':
      return 'draft'
    case 'i':
      return 'inbox'
    case 's':
      return 'send'
    default:
      return null
  }
}

export function useKeyboardNav() {
  // v2.1 phase-5c: conversations read via the RQ-native
  // `useFlatConversations` hook. Optimistic patches (delete / archive /
  // read / pin / star / unread / mark-all-read) are dispatched to the
  // `conversationKeys.infinites()` cache via `patchAllInfiniteLists` —
  // every screen subscribing to that cache line re-renders on the
  // next tick with the mutation applied.
  const { rows: conversations } = useConversationRows()
  const rows = useCurrentListRows()
  const selectedThreadId = useSelectedThreadId()
  const setSelectedThreadId = useSelectThreadId()
  const setComposingNew = useSetAtom(composingNewAtom)
  const setComposeReplySource = useSetAtom(composeReplySourceAtom)
  const setMobileView = useSetAtom(mobileViewAtom)
  const selectList = useSetAtom(selectMailListAtom)
  const setSection = useSetAtom(importanceSectionAtom)
  const setCategory = useSetAtom(categoryFilterAtom)
  const askThread = useSetAtom(threadShortcutAtom)
  const { act } = useConversationActions()
  const folder = useAtomValue(folderAtom)
  const [lastUndo, setLastUndo] = useAtom(lastUndoAtom)
  const setBatchMode = useSetAtom(batchModeAtom)
  const setChecked = useSetAtom(selectedThreadIdsAtom)

  useEffect(() => {
    let gPending = false // for g+i, g+s chord sequences
    function scrollToThread() {
      requestAnimationFrame(() => {
        // list rows mark the pick with `aria-current`
        document.querySelector(`[aria-current="true"]`)?.scrollIntoView({ block: 'nearest' })
      })
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (isEditableTarget(e.target)) return

      // **The chord is resolved before the switch, not inside it.** It
      // was a `default:` arm, and `switch` matches `case 's'` first —
      // so `g s` starred the open thread instead of going to Sent, and
      // left the chord armed because that arm never cleared it. `g a`
      // was advertised in the shortcuts sheet and never existed at
      // all.
      if (gPending) {
        gPending = false
        const list = chordList(e.key)
        if (list) {
          e.preventDefault()
          setSection(null)
          setCategory(null)
          selectList(list)
          return
        }
      }

      switch (e.key) {
        case '#': {
          // the same confirmation every other delete asks for
          if (!selectedThreadId) break
          e.preventDefault()
          askThread({ kind: 'delete', threadId: selectedThreadId })
          break
        }
        case '/': {
          e.preventDefault()
          const searchInput = document.querySelector<HTMLInputElement>(
            'input[placeholder="Search..."]'
          )
          searchInput?.focus()
          break
        }

        case 'ArrowDown':
        // falls through
        case 'j': {
          e.preventDefault()
          if (rows.length === 0) return
          const idx = rows.findIndex((r) => r.threadId === selectedThreadId)
          if (idx < rows.length - 1) {
            setSelectedThreadId(rows[idx + 1]?.threadId ?? null)
            scrollToThread()
          }
          break
        }

        case 'ArrowUp':
        // falls through
        case 'k': {
          e.preventDefault()
          if (rows.length === 0) return
          const idx = rows.findIndex((r) => r.threadId === selectedThreadId)
          if (idx > 0) {
            setSelectedThreadId(rows[idx - 1]?.threadId ?? null)
            scrollToThread()
          }
          break
        }

        case 'c':

        // falls through
        case 'n': {
          e.preventDefault()
          setComposeReplySource(null)
          setComposingNew(true)
          setSelectedThreadId(null)
          setMobileView('thread')
          break
        }

        case 'Enter': {
          if (selectedThreadId !== null) {
            e.preventDefault()
            setMobileView('thread')
          }
          break
        }

        case 'Escape': {
          e.preventDefault()
          setMobileView('list')
          break
        }

        case 'f': {
          if (!selectedThreadId) break
          e.preventDefault()
          setMobileView('thread')
          askThread({ kind: 'forward', threadId: selectedThreadId })
          break
        }

        case 'g': {
          // start chord; the second key is read by `chordList`
          if (gPending) break
          e.preventDefault()
          gPending = true
          setTimeout(() => {
            gPending = false
          }, 1000)
          break
        }
        case 'I': {
          // Shift+I: mark read and go to the next
          if (!selectedThreadId) break
          e.preventDefault()
          void act(selectedThreadId, 'read')
          const readIdx = rows.findIndex((r) => r.threadId === selectedThreadId)
          const nextThread = rows[readIdx + 1]?.threadId ?? rows[readIdx - 1]?.threadId ?? null
          if (nextThread) setSelectedThreadId(nextThread)
          break
        }

        case 'r': {
          if (!selectedThreadId) break
          e.preventDefault()
          setMobileView('thread')
          askThread({ kind: 'reply', threadId: selectedThreadId })
          break
        }

        case 'x': {
          // pick the open conversation for a batch action, or put it back
          if (!selectedThreadId) break
          e.preventDefault()
          const id = selectedThreadId
          setBatchMode(true)
          setChecked((prev) => {
            const next = new Set(prev)
            if (next.has(id)) next.delete(id)
            else next.add(id)
            return next
          })
          break
        }

        case 'z': {
          // take back the last action that has a way back
          if (!lastUndo) break
          e.preventDefault()
          void act(lastUndo.threadId, lastUndo.action)
          setLastUndo(null)
          break
        }

        default: {
          // every other key is the action table's: `e`, `s`, `p`, `b`, `!`,
          // `u`, whichever of a pair applies to the open conversation
          if (!selectedThreadId) break
          const row = conversations.find((c) => c.thread_id === selectedThreadId)
          if (!row) break
          const state = rowStateOf(row, viewOf(folder), Date.now() / 1000)
          const id = actionForKey(keyName(e), state)
          // what asks first (`#`) goes through the reading pane above
          if (!id || ACTIONS[id].confirm) break
          e.preventDefault()
          void act(selectedThreadId, id)
          // archiving takes the row out of the list: the list's own first
          // row becomes the pick
          if (id === 'archive') setSelectedThreadId(null)
          break
        }
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [
    act,
    askThread,
    folder,
    lastUndo,
    setBatchMode,
    setChecked,
    setLastUndo,
    conversations,
    rows,
    selectedThreadId,
    setSelectedThreadId,
    setComposingNew,
    setComposeReplySource,
    setMobileView,
    setCategory,
    selectList,
    setSection,
  ])
}
