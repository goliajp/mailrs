import { useAtom, useSetAtom } from 'jotai'
import { useEffect } from 'react'

import {
  composingNewAtom,
  conversationsAtom,
  mobileViewAtom,
  selectedThreadIdAtom,
  shortcutsDialogOpenAtom,
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
  const [conversations] = useAtom(conversationsAtom)
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
          if (conversations.length === 0) return
          if (selectedThreadId === null) {
            setSelectedThreadId(conversations[0].thread_id)
            return
          }
          const idx = conversations.findIndex((c) => c.thread_id === selectedThreadId)
          if (idx < conversations.length - 1) {
            setSelectedThreadId(conversations[idx + 1].thread_id)
          }
          break
        }

        case 'k':
        case 'ArrowUp': {
          e.preventDefault()
          if (conversations.length === 0) return
          if (selectedThreadId === null) {
            setSelectedThreadId(conversations[0].thread_id)
            return
          }
          const idx = conversations.findIndex((c) => c.thread_id === selectedThreadId)
          if (idx > 0) {
            setSelectedThreadId(conversations[idx - 1].thread_id)
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

        default:
          break
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [conversations, selectedThreadId, setSelectedThreadId, setComposingNew, setMobileView, setShortcutsOpen])
}
