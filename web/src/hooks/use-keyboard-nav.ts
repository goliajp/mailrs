import type { MailListId } from '@/lib/mail-lists'

import { toast } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { useEffect } from 'react'

import {
  useConversationRows,
  useCurrentListRows,
  useSelectedThreadId,
  useSelectThreadId,
} from '@/hooks/use-current-list'
import { queryClient } from '@/lib/query-client'
import { patchAllInfiniteLists } from '@/reducers/snapshot'
import {
  categoryFilterAtom,
  composeReplySourceAtom,
  composingNewAtom,
  importanceSectionAtom,
  mobileViewAtom,
  selectMailListAtom,
  shortcutsDialogOpenAtom,
} from '@/store/ui'
import {
  wireArchiveThread,
  wireBatchMutation,
  wireMarkThreadRead,
  wirePinThread,
  wireStarThread,
  wireUnarchiveThread,
  wireUnpinThread,
  wireUnstarThread,
} from '@/wire/endpoints/mutations'

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
  const setShortcutsOpen = useSetAtom(shortcutsDialogOpenAtom)
  const selectList = useSetAtom(selectMailListAtom)
  const setSection = useSetAtom(importanceSectionAtom)
  const setCategory = useSetAtom(categoryFilterAtom)

  useEffect(() => {
    let gPending = false // for g+i, g+s chord sequences
    function scrollToThread() {
      requestAnimationFrame(() => {
        document.querySelector(`[aria-selected="true"]`)?.scrollIntoView({ block: 'nearest' })
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
          // delete current thread
          if (!selectedThreadId) break
          e.preventDefault()
          wireBatchMutation('delete', [selectedThreadId])
            .then(() => {
              toast.success('Deleted')
              patchAllInfiniteLists(queryClient, (c) =>
                c.thread_id === selectedThreadId ? null : c
              )
              // The pick is cleared rather than moved: with the row
              // gone, the list's own first row is the answer, and
              // nothing here has to guess at the neighbour.
              setSelectedThreadId(null)
            })
            .catch(() => toast.error('Failed'))
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

        case '?': {
          e.preventDefault()
          setShortcutsOpen((prev) => !prev)
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

        case 'e': {
          // archive current thread
          if (!selectedThreadId) break
          e.preventDefault()
          const convo = conversations.find((c) => c.thread_id === selectedThreadId)
          const action = convo?.archived ? 'unarchive' : 'archive'
          const req =
            action === 'archive'
              ? wireArchiveThread(selectedThreadId)
              : wireUnarchiveThread(selectedThreadId)
          req
            .then(() => {
              toast.success(action === 'archive' ? 'Archived' : 'Unarchived')
              patchAllInfiniteLists(queryClient, (c) =>
                c.thread_id === selectedThreadId ? { ...c, archived: action === 'archive' } : c
              )
              // Archiving takes the row out of every list but Archived,
              // so dropping the pick lands on whatever is now first.
              if (action === 'archive') setSelectedThreadId(null)
            })
            .catch(() => toast.error('Failed'))
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
          // forward — focus reply box and switch to forward mode
          if (!selectedThreadId) break
          e.preventDefault()
          setMobileView('thread')
          setTimeout(() => {
            document.querySelectorAll<HTMLButtonElement>('button[aria-pressed]').forEach((btn) => {
              if (btn.textContent === 'Forward') btn.click()
            })
          }, 100)
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
          // Shift+I: mark read and go to next
          if (!selectedThreadId) break
          e.preventDefault()
          wireMarkThreadRead(selectedThreadId).catch(() => {})
          patchAllInfiniteLists(queryClient, (c) =>
            c.thread_id === selectedThreadId ? { ...c, unread_count: 0 } : c
          )
          const readIdx = rows.findIndex((r) => r.threadId === selectedThreadId)
          const nextThread = rows[readIdx + 1]?.threadId ?? rows[readIdx - 1]?.threadId ?? null
          if (nextThread) setSelectedThreadId(nextThread)
          break
        }

        case 'n': {
          e.preventDefault()
          setComposeReplySource(null)
          setComposingNew(true)
          setSelectedThreadId(null)
          setMobileView('thread')
          break
        }

        case 'p': {
          // pin/unpin current thread
          if (!selectedThreadId) break
          e.preventDefault()
          const pinned = conversations.find((c) => c.thread_id === selectedThreadId)?.pinned
          const req = pinned ? wireUnpinThread(selectedThreadId) : wirePinThread(selectedThreadId)
          req
            .then(() => {
              toast.success(pinned ? 'Unpinned' : 'Pinned')
              patchAllInfiniteLists(queryClient, (c) =>
                c.thread_id === selectedThreadId ? { ...c, pinned: !pinned } : c
              )
            })
            .catch(() => toast.error('Failed'))
          break
        }

        case 'r': {
          // focus reply box
          if (!selectedThreadId) break
          e.preventDefault()
          setMobileView('thread')
          // focus the reply editor after a tick
          setTimeout(() => {
            const editor =
              document.querySelector<HTMLElement>('.tiptap.ProseMirror') ??
              document.querySelector<HTMLElement>('[contenteditable="true"]')
            editor?.focus()
          }, 100)
          break
        }

        case 's': {
          // star/unstar current thread
          if (!selectedThreadId) break
          e.preventDefault()
          const flagged = conversations.find((c) => c.thread_id === selectedThreadId)?.flagged
          const nextFlagged = !flagged
          const req = flagged
            ? wireUnstarThread(selectedThreadId)
            : wireStarThread(selectedThreadId)
          req
            .then(() => {
              patchAllInfiniteLists(queryClient, (c) =>
                c.thread_id === selectedThreadId ? { ...c, flagged: nextFlagged } : c
              )
            })
            .catch(() => toast.error('Failed'))
          break
        }

        case 'u': {
          // mark current thread unread
          if (!selectedThreadId) break
          e.preventDefault()
          wireBatchMutation('unread', [selectedThreadId])
            .then(() => {
              toast.success('Marked unread')
              patchAllInfiniteLists(queryClient, (c) =>
                c.thread_id === selectedThreadId
                  ? { ...c, unread_count: Math.max(1, c.unread_count) }
                  : c
              )
            })
            .catch(() => toast.error('Failed'))
          break
        }

        default:
          break
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [
    conversations,
    rows,
    selectedThreadId,
    setSelectedThreadId,
    setComposingNew,
    setComposeReplySource,
    setMobileView,
    setShortcutsOpen,
    setCategory,
    selectList,
    setSection,
  ])
}

// ignore keypresses originating from editable elements
function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName.toLowerCase()
  if (tag === 'input' || tag === 'textarea' || tag === 'select') return true
  if (target.isContentEditable) return true
  return false
}
