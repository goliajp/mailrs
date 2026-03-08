import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { Activity, Inbox, LogOut, Monitor, Moon, Server, Settings, Sun } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { useLocation } from 'react-router'
import type { ThemeMode } from '@/lib/theme'

import { postJson } from '@/lib/api'
import { authAtom } from '@/store/auth'
import { themeAtom } from '@/store/theme'

const THEME_CYCLE: ThemeMode[] = ['system', 'light', 'dark']

const navBtnBase =
  'flex h-9 w-9 items-center justify-center rounded-md transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)]'
const navBtnInactive =
  'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]'
const navBtnActive =
  'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]'

function SidebarLink({
  href,
  icon: Icon,
  label,
  active,
}: {
  href: string
  icon: LucideIcon
  label: string
  active: boolean
}) {
  return (
    <a
      href={href}
      className={`${navBtnBase} ${active ? navBtnActive : navBtnInactive}`}
      title={label}
      aria-label={label}
      aria-current={active ? 'page' : undefined}
    >
      <Icon className="h-5 w-5" />
    </a>
  )
}

export function AppSidebar() {
  const auth = useAtomValue(authAtom)
  const setAuth = useSetAtom(authAtom)
  const [theme, setTheme] = useAtom(themeAtom)
  const { pathname } = useLocation()

  const cycleTheme = () => {
    const idx = THEME_CYCLE.indexOf(theme)
    const next = THEME_CYCLE[(idx + 1) % THEME_CYCLE.length]
    setTheme(next)
  }

  const handleLogout = async () => {
    try {
      await postJson('/auth/logout', {})
    } catch {
      // ignore
    }
    setAuth(null)
    window.location.href = '/login'
  }

  // determine active section from current path
  const section = pathname.startsWith('/admin')
    ? 'server'
    : pathname.startsWith('/protocol')
      ? 'monitor'
      : pathname.startsWith('/settings')
        ? 'settings'
        : 'mail'

  return (
    <aside className="hidden h-full w-14 shrink-0 select-none flex-col items-center border-r border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] py-4 md:flex">
      {/* logo */}
      <div className="mb-4">
        <img src="/icon.svg" alt="mailrs" className="h-9 w-9 rounded-lg" />
      </div>

      {/* nav — mail / server / monitor are parallel top-level sections */}
      <nav className="flex flex-1 flex-col items-center gap-1.5">
        <SidebarLink href="/" icon={Inbox} label="Mail" active={section === 'mail'} />
        <SidebarLink href="/admin" icon={Server} label="Server" active={section === 'server'} />
        <SidebarLink href="/protocol" icon={Activity} label="Monitor" active={section === 'monitor'} />
      </nav>

      {/* bottom actions */}
      <div className="flex flex-col items-center gap-2">
        <button
          onClick={cycleTheme}
          className={`${navBtnBase} ${navBtnInactive}`}
          title={`Theme: ${theme}`}
          aria-label={`Switch theme, current: ${theme}`}
        >
          {theme === 'dark' ? (
            <Moon className="h-5 w-5" />
          ) : theme === 'light' ? (
            <Sun className="h-5 w-5" />
          ) : (
            <Monitor className="h-5 w-5" />
          )}
        </button>

        <SidebarLink href="/settings" icon={Settings} label="Settings" active={section === 'settings'} />

        <button
          onClick={handleLogout}
          className={`${navBtnBase} ${navBtnInactive}`}
          title={`Sign out (${auth?.address})`}
          aria-label={`Sign out (${auth?.address})`}
        >
          <LogOut className="h-5 w-5" />
        </button>
      </div>
    </aside>
  )
}
