import type { Editor } from '@tiptap/react'

import { useCallback, useEffect, useRef, useState } from 'react'

import { fetchList } from '@/lib/api'
import { escapeHtml } from '@/lib/html-utils'

type Contact = { email: string; name: string; raw: string }

type Trigger = {
  // absolute editor positions covering "@query" (including the @)
  from: number
  query: string
  to: number
  // viewport coords at the trigger for popover placement
  x: number
  y: number
}

// Gmail-style @-mention picker. when the user types "@" (at the start of a
// line or after whitespace) and keeps typing, we fetch matching contacts and
// show a dropdown anchored to the cursor. selecting a contact replaces
// "@query" with a standard mailto link so the outgoing HTML is portable
export function MentionPicker({ editor }: { editor: Editor | null }) {
  const [trigger, setTrigger] = useState<null | Trigger>(null)
  const [contacts, setContacts] = useState<Contact[]>([])
  const [activeIndex, setActiveIndex] = useState(0)
  const debounceRef = useRef<null | ReturnType<typeof setTimeout>>(null)
  const lastQueryRef = useRef<string>('')

  // detect "@query" around the cursor after every editor transaction
  useEffect(() => {
    if (!editor) return
    const update = () => {
      const { state } = editor
      if (!state.selection.empty) {
        setTrigger(null)
        return
      }
      const cursor = state.selection.from
      const $pos = state.doc.resolve(cursor)
      // textBetween of the current block up to the cursor — scoped so we
      // don't match an "@" three paragraphs up
      const blockStart = cursor - $pos.parentOffset
      const before = state.doc.textBetween(blockStart, cursor, '\n', '\0')
      const at = before.lastIndexOf('@')
      if (at === -1) {
        setTrigger(null)
        return
      }
      // "@" must be the first char of the line or preceded by whitespace;
      // otherwise this is part of an email address the user is typing
      const prev = at > 0 ? before[at - 1] : ''
      if (prev && !/\s/.test(prev)) {
        setTrigger(null)
        return
      }
      const query = before.slice(at + 1)
      // a space after the "@" means the mention window has closed
      if (/\s/.test(query)) {
        setTrigger(null)
        return
      }
      const from = blockStart + at
      const to = cursor
      // anchor coords from ProseMirror
      let coords: { bottom: number; left: number; top: number }
      try {
        coords = editor.view.coordsAtPos(from)
      } catch {
        setTrigger(null)
        return
      }
      setTrigger({ from, query, to, x: coords.left, y: coords.bottom })
    }
    editor.on('selectionUpdate', update)
    editor.on('update', update)
    return () => {
      editor.off('selectionUpdate', update)
      editor.off('update', update)
    }
  }, [editor])

  // debounced fetch whenever the query changes
  useEffect(() => {
    if (!trigger) {
      if (debounceRef.current) clearTimeout(debounceRef.current)
      setContacts([])
      return
    }
    const query = trigger.query
    lastQueryRef.current = query
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(async () => {
      try {
        const results = await fetchList<string>(`/contacts?q=${encodeURIComponent(query)}&limit=8`)
        // ignore out-of-order responses
        if (lastQueryRef.current !== query) return
        setContacts(results.map(parseContact))
        setActiveIndex(0)
      } catch {
        setContacts([])
      }
    }, 150)
  }, [trigger])

  const insertMention = useCallback(
    (c: Contact) => {
      if (!editor || !trigger) return
      const displayName = c.name || c.email
      const link = `<a href="mailto:${escapeHtml(c.email)}">${escapeHtml(displayName)}</a>`
      editor
        .chain()
        .focus()
        .insertContentAt({ from: trigger.from, to: trigger.to }, link + '&nbsp;')
        .run()
      setTrigger(null)
    },
    [editor, trigger]
  )

  // keyboard navigation while the dropdown is open
  useEffect(() => {
    if (!trigger || contacts.length === 0) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setActiveIndex((i) => (i + 1) % contacts.length)
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        setActiveIndex((i) => (i - 1 + contacts.length) % contacts.length)
      } else if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault()
        const picked = contacts[activeIndex]
        if (picked) insertMention(picked)
      } else if (e.key === 'Escape') {
        e.preventDefault()
        setTrigger(null)
      }
    }
    // capture-phase so tiptap's own Enter handler doesn't fire first
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [trigger, contacts, activeIndex, insertMention])

  if (!trigger || contacts.length === 0) return null

  return (
    <div
      className="border-border bg-surface fixed z-50 max-h-60 min-w-48 overflow-y-auto rounded-lg border shadow-lg"
      style={{ left: `${trigger.x}px`, top: `${trigger.y + 4}px` }}
    >
      {contacts.map((c, i) => (
        <button
          className={`flex w-full flex-col gap-0.5 px-3 py-1.5 text-left transition-colors ${
            i === activeIndex
              ? 'bg-accent/10 text-accent'
              : 'text-fg-secondary hover:bg-bg-secondary'
          }`}
          key={c.raw}
          onPointerDown={(e) => {
            // prevent editor blur before we've read the selection
            e.preventDefault()
            insertMention(c)
          }}
        >
          <span className="text-fg truncate text-sm">{c.name || c.email}</span>
          {c.name && <span className="text-fg-muted truncate text-xs">{c.email}</span>}
        </button>
      ))}
    </div>
  )
}

// split "Display Name" <addr@host> / Display <addr> / bare addr into parts
function parseContact(raw: string): Contact {
  const angle = raw.match(/^\s*(?:"([^"]*)"|([^<]*?))\s*<([^>]+)>\s*$/)
  if (angle) {
    const name = (angle[1] ?? angle[2] ?? '').trim()
    const email = angle[3].trim()
    return { email, name, raw }
  }
  // bare address
  return { email: raw.trim(), name: '', raw }
}
