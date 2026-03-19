import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useEffect } from 'react'
import { toast } from 'sonner'

import { postJson } from '@/lib/api'
import {
  composingNewAtom,
  conversationsAtom,
  mobileViewAtom,
  selectedThreadIdAtom,
  shortcutsDialogOpenAtom,
  visibleConversationIdsAtom,
} from '@/store/chat'

// ignore keypresses originating from editable elements
function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName.toLowerCase()
  if (tag === 'input' || tag === 'textarea' || tag === 'select') return true
  if (target.isContentEditable) return true
  return false
}

export function useKeyboardNav() {
  const [conversations, setConversations] = useAtom(conversationsAtom)
  const visibleIds = useAtomValue(visibleConversationIdsAtom)
  const [selectedThreadId, setSelectedThreadId] = useAtom(selectedThreadIdAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const setMobileView = useSetAtom(mobileViewAtom)
  const setShortcutsOpen = useSetAtom(shortcutsDialogOpenAtom)

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (isEditableTarget(e.target)) return

      switch (e.key) {
        case 'j':
        case 'ArrowDown': {
          e.preventDefault()
          if (visibleIds.length === 0) return
          if (selectedThreadId === null) {
            setSelectedThreadId(visibleIds[0])
            return
          }
          const idx = visibleIds.indexOf(selectedThreadId)
          if (idx < visibleIds.length - 1) {
            setSelectedThreadId(visibleIds[idx + 1])
          }
          break
        }

        case 'k':
        case 'ArrowUp': {
          e.preventDefault()
          if (visibleIds.length === 0) return
          if (selectedThreadId === null) {
            setSelectedThreadId(visibleIds[0])
            return
          }
          const idx = visibleIds.indexOf(selectedThreadId)
          if (idx > 0) {
            setSelectedThreadId(visibleIds[idx - 1])
          }
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

        case 'n': {
          e.preventDefault()
          setComposingNew(true)
          setSelectedThreadId(null)
          setMobileView('thread')
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

        case 'e': {
          // archive current thread
          if (!selectedThreadId) break
          e.preventDefault()
          const convo = conversations.find((c) => c.thread_id === selectedThreadId)
          const action = convo?.archived ? 'unarchive' : 'archive'
          postJson(`/conversations/${encodeURIComponent(selectedThreadId)}/${action}`, {})
            .then(() => {
              toast.success(action === 'archive' ? 'Archived' : 'Unarchived')
              setConversations((prev) =>
                prev.map((c) =>
                  c.thread_id === selectedThreadId ? { ...c, archived: action === 'archive' } : c
                )
              )
            })
            .catch(() => toast.error('Failed'))
          break
        }

        case 's': {
          // star/unstar current thread
          if (!selectedThreadId) break
          e.preventDefault()
          const flagged = conversations.find((c) => c.thread_id === selectedThreadId)?.flagged
          const act = flagged ? 'unstar' : 'star'
          postJson(`/conversations/${encodeURIComponent(selectedThreadId)}/${act}`, {})
            .then(() => {
              setConversations((prev) =>
                prev.map((c) =>
                  c.thread_id === selectedThreadId ? { ...c, flagged: act === 'star' } : c
                )
              )
            })
            .catch(() => toast.error('Failed'))
          break
        }

        case 'u': {
          // mark current thread unread
          if (!selectedThreadId) break
          e.preventDefault()
          postJson(`/conversations/batch`, { thread_ids: [selectedThreadId], action: 'unread' })
            .then(() => {
              toast.success('Marked unread')
              setConversations((prev) =>
                prev.map((c) =>
                  c.thread_id === selectedThreadId ? { ...c, unread_count: Math.max(1, c.unread_count) } : c
                )
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
            const editor = document.querySelector<HTMLElement>('.tiptap.ProseMirror')
              ?? document.querySelector<HTMLElement>('[contenteditable="true"]')
            editor?.focus()
          }, 100)
          break
        }

        case '#': {
          // delete current thread
          if (!selectedThreadId) break
          e.preventDefault()
          postJson(`/conversations/batch`, { thread_ids: [selectedThreadId], action: 'delete' })
            .then(() => {
              toast.success('Deleted')
              setConversations((prev) => prev.filter((c) => c.thread_id !== selectedThreadId))
              // move to next conversation
              const idx = visibleIds.indexOf(selectedThreadId)
              const next = visibleIds[idx + 1] ?? visibleIds[idx - 1] ?? null
              setSelectedThreadId(next)
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
  }, [conversations, visibleIds, selectedThreadId, setSelectedThreadId, setComposingNew, setConversations, setMobileView, setShortcutsOpen])
}
