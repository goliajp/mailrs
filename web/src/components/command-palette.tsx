import { useAtomValue, useSetAtom } from 'jotai'
import {
  Home,
  Inbox,
  LogOut,
  Monitor,
  Moon,
  PenSquare,
  Search,
  Send,
  Settings,
  Shield,
  Sun,
} from 'lucide-react'
import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'

import { MobileModal } from '@/components/mobile-modal'
import { authAtom } from '@/store/auth'
import { themeModeAtom } from '@/store/theme'
import { composeReplySourceAtom, composingNewAtom } from '@/store/ui'

type Command = {
  action: () => void
  category: 'Actions' | 'Navigation' | 'Search'
  icon: React.ReactNode
  id: string
  label: string
  shortcut?: string
}

type GroupedCommands = {
  category: string
  commands: Command[]
}

export function CommandPalette() {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const listboxId = useId()

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
    <MobileModal className="items-start" onClose={close} open>
      <div
        className="border-border bg-surface mx-auto mt-[20vh] max-w-lg overflow-hidden rounded-xl border shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* search input */}
        <div className="border-border flex items-center border-b px-4">
          <Search aria-hidden="true" className="text-fg-muted shrink-0" size={18} />
          <input
            aria-activedescendant={commands[selectedIndex]?.id}
            aria-autocomplete="list"
            aria-controls={listboxId}
            aria-expanded
            aria-label="Search commands"
            autoComplete="off"
            className="text-fg placeholder:text-fg-muted w-full bg-transparent px-3 py-3 text-lg outline-none"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Type a command or search..."
            ref={inputRef}
            role="combobox"
            type="text"
            value={query}
          />
          <kbd
            aria-hidden="true"
            className="border-border text-fg-muted shrink-0 rounded border px-1.5 py-0.5 text-xs"
          >
            ESC
          </kbd>
        </div>

        {/* results */}
        <div
          aria-label="Commands"
          className="max-h-80 overflow-y-auto py-2"
          id={listboxId}
          ref={listRef}
          role="listbox"
        >
          {commands.length === 0 ? (
            <div className="text-fg-muted px-4 py-8 text-center text-sm">No results found</div>
          ) : (
            groups.map((group) => (
              <div key={group.category} role="group">
                <div className="text-fg-muted px-4 py-1.5 text-xs font-medium tracking-wider uppercase">
                  {group.category}
                </div>
                {group.commands.map((cmd) => {
                  const idx = flatIndex++
                  const isSelected = idx === selectedIndex

                  return (
                    <button
                      aria-selected={isSelected}
                      className={`flex w-full cursor-pointer items-center gap-3 px-4 py-2.5 text-left transition-colors ${
                        isSelected
                          ? 'bg-accent/10 text-fg'
                          : 'text-fg-secondary hover:bg-bg-secondary'
                      }`}
                      data-selected={isSelected}
                      id={cmd.id}
                      key={cmd.id}
                      onClick={() => cmd.action()}
                      onMouseEnter={() => setSelectedIndex(idx)}
                      role="option"
                      type="button"
                    >
                      <span aria-hidden="true" className="text-fg-muted shrink-0">
                        {cmd.icon}
                      </span>
                      <span className="flex-1 text-sm">{cmd.label}</span>
                      {cmd.shortcut && (
                        <kbd
                          aria-hidden="true"
                          className="border-border text-fg-muted rounded border px-1.5 py-0.5 text-xs"
                        >
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
    </MobileModal>
  )
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

function useCommands(query: string, onClose: () => void): Command[] {
  const navigate = useNavigate()
  const setAuth = useSetAtom(authAtom)
  const setComposing = useSetAtom(composingNewAtom)
  const setComposeReplySource = useSetAtom(composeReplySourceAtom)
  const setTheme = useSetAtom(themeModeAtom)
  const theme = useAtomValue(themeModeAtom)

  const staticCommands = useMemo<Command[]>(
    () => [
      {
        category: 'Navigation',
        icon: <Home size={16} />,
        id: 'nav-home',
        label: 'Go to Home',
        action: () => {
          navigate('/')
          onClose()
        },
      },
      {
        category: 'Navigation',
        icon: <Inbox size={16} />,
        id: 'nav-inbox',
        label: 'Go to Inbox',
        action: () => {
          navigate('/mail')
          onClose()
        },
      },
      {
        category: 'Navigation',
        icon: <Send size={16} />,
        id: 'nav-sent',
        label: 'Go to Sent',
        action: () => {
          navigate('/mail?folder=Sent')
          onClose()
        },
      },
      {
        category: 'Navigation',
        icon: <Settings size={16} />,
        id: 'nav-settings',
        label: 'Go to Settings',
        shortcut: '',
        action: () => {
          navigate('/settings')
          onClose()
        },
      },
      {
        category: 'Navigation',
        icon: <Shield size={16} />,
        id: 'nav-admin',
        label: 'Go to Admin',
        action: () => {
          navigate('/admin')
          onClose()
        },
      },
      {
        category: 'Navigation',
        icon: <Monitor size={16} />,
        id: 'nav-protocol',
        label: 'Go to Protocol Monitor',
        action: () => {
          navigate('/protocol')
          onClose()
        },
      },
      {
        category: 'Actions',
        icon: <PenSquare size={16} />,
        id: 'action-compose',
        label: 'Compose New Email',
        shortcut: 'C',
        action: () => {
          setComposeReplySource(null)
          setComposing(true)
          navigate('/mail')
          onClose()
        },
      },
      {
        category: 'Actions',
        icon: theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />,
        id: 'action-toggle-theme',
        label: 'Toggle Theme',
        action: () => {
          const next = theme === 'dark' ? 'light' : theme === 'light' ? 'system' : 'dark'
          setTheme(next)
          onClose()
        },
      },
      {
        category: 'Actions',
        icon: <Moon size={16} />,
        id: 'action-dark-mode',
        label: 'Toggle Dark Mode',
        action: () => {
          setTheme(theme === 'dark' ? 'light' : 'dark')
          onClose()
        },
      },
      {
        category: 'Actions',
        icon: <LogOut size={16} />,
        id: 'action-logout',
        label: 'Logout',
        action: () => {
          if (!window.confirm('Sign out? You will need to sign in again to access your mailbox.'))
            return
          setAuth(null)
          navigate('/login')
          onClose()
        },
      },
    ],
    [navigate, onClose, setAuth, setComposeReplySource, setComposing, setTheme, theme]
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
          category: 'Search' as const,
          icon: <Search size={16} />,
          id: 'search-query',
          label: `Search emails for: ${query.trim()}`,
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
