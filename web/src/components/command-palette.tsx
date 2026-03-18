import { useAtomValue, useSetAtom } from 'jotai'
import {
  LogOut,
  Mail,
  Monitor,
  Moon,
  PenSquare,
  Search,
  Send,
  Settings,
  Shield,
  Sun,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'

import { authAtom } from '@/store/auth'
import { composingNewAtom } from '@/store/chat'
import { themeAtom } from '@/store/theme'

type Command = {
  id: string
  label: string
  icon: React.ReactNode
  category: 'Navigation' | 'Actions' | 'Search'
  shortcut?: string
  action: () => void
}

function useCommands(query: string, onClose: () => void): Command[] {
  const navigate = useNavigate()
  const setAuth = useSetAtom(authAtom)
  const setComposing = useSetAtom(composingNewAtom)
  const setTheme = useSetAtom(themeAtom)
  const theme = useAtomValue(themeAtom)

  const staticCommands = useMemo<Command[]>(
    () => [
      {
        id: 'nav-home',
        label: 'Go to Home',
        icon: <Mail size={16} />,
        category: 'Navigation',
        action: () => {
          navigate('/')
          onClose()
        },
      },
      {
        id: 'nav-inbox',
        label: 'Go to Inbox',
        icon: <Mail size={16} />,
        category: 'Navigation',
        action: () => {
          navigate('/mail')
          onClose()
        },
      },
      {
        id: 'nav-sent',
        label: 'Go to Sent',
        icon: <Send size={16} />,
        category: 'Navigation',
        action: () => {
          navigate('/mail?folder=Sent')
          onClose()
        },
      },
      {
        id: 'nav-settings',
        label: 'Go to Settings',
        icon: <Settings size={16} />,
        category: 'Navigation',
        shortcut: '',
        action: () => {
          navigate('/settings')
          onClose()
        },
      },
      {
        id: 'nav-admin',
        label: 'Go to Admin',
        icon: <Shield size={16} />,
        category: 'Navigation',
        action: () => {
          navigate('/admin')
          onClose()
        },
      },
      {
        id: 'nav-protocol',
        label: 'Go to Protocol Monitor',
        icon: <Monitor size={16} />,
        category: 'Navigation',
        action: () => {
          navigate('/protocol')
          onClose()
        },
      },
      {
        id: 'action-compose',
        label: 'Compose New Email',
        icon: <PenSquare size={16} />,
        category: 'Actions',
        shortcut: 'C',
        action: () => {
          setComposing(true)
          navigate('/mail')
          onClose()
        },
      },
      {
        id: 'action-toggle-theme',
        label: 'Toggle Theme',
        icon: theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />,
        category: 'Actions',
        action: () => {
          const next = theme === 'dark' ? 'light' : theme === 'light' ? 'system' : 'dark'
          setTheme(next)
          onClose()
        },
      },
      {
        id: 'action-dark-mode',
        label: 'Toggle Dark Mode',
        icon: <Moon size={16} />,
        category: 'Actions',
        action: () => {
          setTheme(theme === 'dark' ? 'light' : 'dark')
          onClose()
        },
      },
      {
        id: 'action-logout',
        label: 'Logout',
        icon: <LogOut size={16} />,
        category: 'Actions',
        action: () => {
          if (!window.confirm('Sign out? You will need to sign in again to access your mailbox.')) return
          setAuth(null)
          navigate('/login')
          onClose()
        },
      },
    ],
    [navigate, onClose, setAuth, setComposing, setTheme, theme]
  )

  return useMemo(() => {
    const trimmed = query.trim().toLowerCase()

    const filtered =
      trimmed === ''
        ? staticCommands
        : staticCommands.filter((cmd) => cmd.label.toLowerCase().includes(trimmed))

    // add dynamic search command when there's a query
    if (trimmed !== '') {
      return [
        ...filtered,
        {
          id: 'search-query',
          label: `Search emails for: ${query.trim()}`,
          icon: <Search size={16} />,
          category: 'Search' as const,
          action: () => {
            navigate(`/?q=${encodeURIComponent(query.trim())}`)
            onClose()
          },
        },
      ]
    }

    return filtered
  }, [query, staticCommands, navigate, onClose])
}

type GroupedCommands = {
  category: string
  commands: Command[]
}

function groupByCategory(commands: Command[]): GroupedCommands[] {
  const order: Command['category'][] = ['Navigation', 'Actions', 'Search']
  const groups: GroupedCommands[] = []

  for (const category of order) {
    const cmds = commands.filter((c) => c.category === category)
    if (cmds.length > 0) {
      groups.push({ category, commands: cmds })
    }
  }

  return groups
}

export function CommandPalette() {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  const close = useCallback(() => {
    setOpen(false)
    setQuery('')
    setSelectedIndex(0)
  }, [])

  const commands = useCommands(query, close)
  const groups = useMemo(() => groupByCategory(commands), [commands])

  // global keyboard listener
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault()
        setOpen((prev) => !prev)
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [])

  // focus input when opened
  useEffect(() => {
    if (open) {
      requestAnimationFrame(() => inputRef.current?.focus())
    }
  }, [open])

  // reset selected index when commands change
  useEffect(() => {
    setSelectedIndex(0)
  }, [commands.length])

  // scroll selected item into view
  useEffect(() => {
    if (!listRef.current) return
    const selected = listRef.current.querySelector('[data-selected="true"]')
    selected?.scrollIntoView({ block: 'nearest' })
  }, [selectedIndex])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        close()
        return
      }

      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setSelectedIndex((i) => (i + 1) % commands.length)
        return
      }

      if (e.key === 'ArrowUp') {
        e.preventDefault()
        setSelectedIndex((i) => (i - 1 + commands.length) % commands.length)
        return
      }

      if (e.key === 'Enter') {
        e.preventDefault()
        commands[selectedIndex]?.action()
      }
    },
    [commands, selectedIndex, close]
  )

  if (!open) return null

  let flatIndex = 0

  return (
    <div className="fixed inset-0 z-50 bg-black/50 backdrop-blur-sm" onClick={close}>
      <div
        className="mx-auto mt-[20vh] max-w-lg overflow-hidden rounded-xl border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* search input */}
        <div className="flex items-center border-b border-[var(--color-border-default)] px-4">
          <Search size={18} className="shrink-0 text-[var(--color-text-tertiary)]" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Type a command or search..."
            className="w-full bg-transparent px-3 py-3 text-lg text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)]"
          />
          <kbd className="shrink-0 rounded border border-[var(--color-border-default)] px-1.5 py-0.5 text-xs text-[var(--color-text-tertiary)]">
            ESC
          </kbd>
        </div>

        {/* results */}
        <div ref={listRef} className="max-h-80 overflow-y-auto py-2">
          {commands.length === 0 ? (
            <div className="px-4 py-8 text-center text-sm text-[var(--color-text-tertiary)]">
              No results found
            </div>
          ) : (
            groups.map((group) => (
              <div key={group.category}>
                <div className="px-4 py-1.5 text-xs font-medium uppercase tracking-wider text-[var(--color-text-tertiary)]">
                  {group.category}
                </div>
                {group.commands.map((cmd) => {
                  const idx = flatIndex++
                  const isSelected = idx === selectedIndex

                  return (
                    <button
                      key={cmd.id}
                      data-selected={isSelected}
                      className={`flex w-full cursor-pointer items-center gap-3 px-4 py-2.5 text-left transition-colors ${
                        isSelected
                          ? 'bg-[var(--color-bg-selected)] text-[var(--color-text-primary)]'
                          : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-hover)]'
                      }`}
                      onClick={() => cmd.action()}
                      onMouseEnter={() => setSelectedIndex(idx)}
                    >
                      <span className="shrink-0 text-[var(--color-text-tertiary)]">
                        {cmd.icon}
                      </span>
                      <span className="flex-1 text-sm">{cmd.label}</span>
                      {cmd.shortcut && (
                        <kbd className="rounded border border-[var(--color-border-default)] px-1.5 py-0.5 text-xs text-[var(--color-text-tertiary)]">
                          {cmd.shortcut}
                        </kbd>
                      )}
                    </button>
                  )
                })}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  )
}
