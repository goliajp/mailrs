import { toast } from '@goliapkg/gds'
import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useEffect } from 'react'

import { useCurrentMailFilters } from '@/hooks/use-current-mail-filters'
import { useFlatConversations } from '@/hooks/use-flat-conversations'
import { queryClient } from '@/lib/query-client'
import { patchAllInfiniteLists } from '@/reducers/snapshot'
import {
  categoryFilterAtom,
  composeReplySourceAtom,
  composingNewAtom,
  folderAtom,
  importanceSectionAtom,
  mobileViewAtom,
  quickFilterAtom,
  selectedThreadIdAtom,
  shortcutsDialogOpenAtom,
  visibleConversationIdsAtom,
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

export function useKeyboardNav() {
  // v2.1 phase-5c: conversations read via the RQ-native
  // `useFlatConversations` hook. Optimistic patches (delete / archive /
  // read / pin / star / unread / mark-all-read) are dispatched to the
  // `conversationKeys.infinites()` cache via `patchAllInfiniteLists` —
  // every screen subscribing to that cache line re-renders on the
  // next tick with the mutation applied.
  const filters = useCurrentMailFilters()
  const { conversations } = useFlatConversations(filters)
  const visibleIds = useAtomValue(visibleConversationIdsAtom)
  const [selectedThreadId, setSelectedThreadId] = useAtom(selectedThreadIdAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const setComposeReplySource = useSetAtom(composeReplySourceAtom)
  const setMobileView = useSetAtom(mobileViewAtom)
  const setShortcutsOpen = useSetAtom(shortcutsDialogOpenAtom)
  const setFolder = useSetAtom(folderAtom)
  const setSection = useSetAtom(importanceSectionAtom)
  const setQuickFilter = useSetAtom(quickFilterAtom)
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
              const idx = visibleIds.indexOf(selectedThreadId)
              const next = visibleIds[idx + 1] ?? visibleIds[idx - 1] ?? null
              setSelectedThreadId(next)
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
          if (visibleIds.length === 0) return
          if (selectedThreadId === null) {
            setSelectedThreadId(visibleIds[0])
            scrollToThread()
            return
          }
          const idx = visibleIds.indexOf(selectedThreadId)
          if (idx < visibleIds.length - 1) {
            setSelectedThreadId(visibleIds[idx + 1])
            scrollToThread()
          }
          break
        }

        case 'ArrowUp':
        // falls through
        case 'k': {
          e.preventDefault()
          if (visibleIds.length === 0) return
          if (selectedThreadId === null) {
            setSelectedThreadId(visibleIds[0])
            scrollToThread()
            return
          }
          const idx = visibleIds.indexOf(selectedThreadId)
          if (idx > 0) {
            setSelectedThreadId(visibleIds[idx - 1])
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
              // auto-advance to next thread after archive
              if (action === 'archive') {
                const archIdx = visibleIds.indexOf(selectedThreadId)
                const nextId = visibleIds[archIdx + 1] ?? visibleIds[archIdx - 1] ?? null
                if (nextId) setSelectedThreadId(nextId)
              }
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
          // start chord: g+i = inbox, g+s = sent, g+a = action
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
          const readIdx = visibleIds.indexOf(selectedThreadId)
          const nextThread = visibleIds[readIdx + 1] ?? visibleIds[readIdx - 1] ?? null
          if (nextThread) setSelectedThreadId(nextThread)
          break
        }

        case 'i': {
          if (!gPending) break
          e.preventDefault()
          gPending = false
          setFolder(null)
          setSection(null)
          setQuickFilter('all')
          setCategory(null)
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
          if (gPending && e.key === 's') {
            e.preventDefault()
            gPending = false
            setFolder('Sent')
            setSection(null)
            setQuickFilter('all')
            setCategory(null)
          } else if (gPending && e.key === 'a') {
            e.preventDefault()
            gPending = false
            setFolder(null)
            setSection('action')
            setQuickFilter('all')
            setCategory(null)
          } else {
            gPending = false
          }
          break
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [
    conversations,
    visibleIds,
    selectedThreadId,
    setSelectedThreadId,
    setComposingNew,
    setComposeReplySource,
    setMobileView,
    setShortcutsOpen,
    setCategory,
    setFolder,
    setQuickFilter,
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
