import type { ThemeMode } from '@/lib/theme'
import type { LucideIcon } from 'lucide-react'

import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import {
  Activity,
  Home,
  Inbox,
  LogOut,
  Monitor,
  Moon,
  Server,
  Settings,
  Sun,
} from 'lucide-react'
import { useEffect, useState } from 'react'
import { useLocation } from 'react-router'

import { postJson } from '@/lib/api'
import { cn } from '@/lib/cn'
import { authAtom } from '@/store/auth'
import { selectedDomainsAtom, unreadCountAtom } from '@/store/chat'
import { themeAtom } from '@/store/theme'

const THEME_CYCLE: ThemeMode[] = ['system', 'light', 'dark']

const navBtnBase =
  'flex h-9 w-9 items-center justify-center rounded-md transition-all duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus-ring)]'
const navBtnInactive =
  'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]'
const navBtnActive =
  'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]'

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

  const unreadCount = useAtomValue(unreadCountAtom)
  const [showLogoutConfirm, setShowLogoutConfirm] = useState(false)
  const [selectedDomains, setSelectedDomains] = useAtom(selectedDomainsAtom)
  const domains = auth?.accessible_domains ?? []

  const doLogout = async () => {
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
        : pathname.startsWith('/mail')
          ? 'mail'
          : 'home'

  return (
    <>
      {/* desktop: vertical sidebar */}
      <aside className="hidden h-full w-14 shrink-0 flex-col items-center pt-1.5 pb-4 select-none md:flex">
        {/* logo */}
        <div className="mb-3">
          <img alt="Mailrs" className="h-7 w-7 rounded-md" src="/icon.svg" />
        </div>

        {/* home */}
        <SidebarLink
          active={section === 'home'}
          href="/"
          icon={Home}
          label="Home"
        />

        {/* inbox + domain group */}
        <div className="mt-1 flex flex-col items-center gap-0.5">
          <SidebarLink
            active={section === 'mail' && selectedDomains.length === 0}
            badge={unreadCount}
            href="/mail"
            icon={Inbox}
            label="Mail"
          />
          {domains.length > 0 &&
            domains.map((d) => {
              const active =
                section === 'mail' &&
                selectedDomains.length === 1 &&
                selectedDomains[0] === d
              const label = d.split('.')[0]
              return (
                <button
                  className={cn(
                    navBtnBase,
                    active ? navBtnActive : navBtnInactive,
                    'h-8 w-8 text-[9px] font-semibold'
                  )}
                  key={d}
                  onClick={() => setSelectedDomains(active ? [] : [d])}
                  title={d}
                >
                  {label.slice(0, 3)}
                </button>
              )
            })}
        </div>

        {/* separator */}
        <div className="my-2 h-px w-8 bg-[var(--color-border-default)]" />

        {/* other nav */}
        <nav className="flex flex-col items-center gap-1.5">
          <SidebarLink
            active={section === 'server'}
            href="/admin"
            icon={Server}
            label="Server"
          />
          <SidebarLink
            active={section === 'monitor'}
            href="/protocol"
            icon={Activity}
            label="Monitor"
          />
        </nav>

        <div className="flex-1" />

        {/* bottom actions */}
        <div className="flex flex-col items-center gap-2">
          <button
            aria-label={`Switch theme, current: ${theme}`}
            className={`${navBtnBase} ${navBtnInactive}`}
            onClick={cycleTheme}
            title={`Theme: ${theme}`}
          >
            {theme === 'dark' ? (
              <Moon className="h-5 w-5" />
            ) : theme === 'light' ? (
              <Sun className="h-5 w-5" />
            ) : (
              <Monitor className="h-5 w-5" />
            )}
          </button>

          <SidebarLink
            active={section === 'settings'}
            href="/settings"
            icon={Settings}
            label="Settings"
          />

          <button
            aria-label={`Sign out (${auth?.address})`}
            className={`${navBtnBase} ${navBtnInactive}`}
            onClick={() => setShowLogoutConfirm(true)}
            title={`Sign out (${auth?.address})`}
          >
            <LogOut className="h-5 w-5" />
          </button>
        </div>

        {showLogoutConfirm && (
          <LogoutConfirmDialog
            onCancel={() => setShowLogoutConfirm(false)}
            onConfirm={doLogout}
          />
        )}
      </aside>

      {/* mobile: bottom tab bar */}
      <nav className="flex items-stretch border-t border-[var(--color-border-default)] bg-[var(--color-bg-raised)] select-none md:hidden">
        <MobileNavLink
          active={section === 'home'}
          href="/"
          icon={Home}
          label="Home"
        />
        <MobileNavLink
          active={section === 'mail'}
          badge={unreadCount}
          href="/mail"
          icon={Inbox}
          label="Mail"
        />
        <MobileNavLink
          active={section === 'server'}
          href="/admin"
          icon={Server}
          label="Admin"
        />
        <MobileNavLink
          active={section === 'settings'}
          href="/settings"
          icon={Settings}
          label="Settings"
        />
        <button
          aria-label="Sign out"
          className="flex flex-1 flex-col items-center gap-0.5 py-1.5 text-[10px] text-[var(--color-text-tertiary)] transition-colors"
          onClick={() => setShowLogoutConfirm(true)}
        >
          <LogOut className="h-5 w-5" />
          <span>Sign out</span>
        </button>

        {showLogoutConfirm && (
          <LogoutConfirmDialog
            onCancel={() => setShowLogoutConfirm(false)}
            onConfirm={doLogout}
          />
        )}
      </nav>
    </>
  )
}

function LogoutConfirmDialog({
  onCancel,
  onConfirm,
}: {
  onCancel: () => void
  onConfirm: () => void
}) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel()
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [onCancel])

  return (
    <div
      aria-modal="true"
      className="fixed inset-0 z-50 flex animate-[fadeIn_150ms_ease-out] items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={onCancel}
      role="dialog"
    >
      <div
        className="mx-4 w-full max-w-sm animate-[scaleIn_150ms_ease-out] rounded-lg border border-[var(--color-border-default)] bg-[var(--color-bg-raised)] p-6 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="text-base font-semibold text-[var(--color-text-primary)]">
          Sign out?
        </h3>
        <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
          You will need to sign in again to access your mailbox.
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <button
            className="rounded-md px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-hover)]"
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            className="rounded-md bg-[var(--color-status-danger)] px-3 py-1.5 text-sm font-medium text-white transition-colors hover:opacity-90"
            onClick={onConfirm}
          >
            Sign out
          </button>
        </div>
      </div>
    </div>
  )
}

// mobile bottom nav link
function MobileNavLink({
  active,
  badge,
  href,
  icon: Icon,
  label,
}: {
  active: boolean
  badge?: number
  href: string
  icon: LucideIcon
  label: string
}) {
  return (
    <a
      aria-current={active ? 'page' : undefined}
      aria-label={label}
      className={cn(
        'relative flex flex-1 flex-col items-center gap-0.5 py-1.5 text-[10px] transition-colors',
        active
          ? 'text-[var(--color-brand-primary)]'
          : 'text-[var(--color-text-tertiary)]'
      )}
      href={href}
    >
      <Icon className="h-5 w-5" />
      <span>{label}</span>
      {badge != null && badge > 0 && (
        <span className="absolute top-0.5 left-1/2 ml-2 flex h-4 min-w-4 items-center justify-center rounded-full bg-[var(--color-status-danger)] px-0.5 text-[9px] leading-none font-bold text-white">
          {badge > 99 ? '99+' : badge}
        </span>
      )}
    </a>
  )
}

function SidebarLink({
  active,
  badge,
  href,
  icon: Icon,
  label,
}: {
  active: boolean
  badge?: number
  href: string
  icon: LucideIcon
  label: string
}) {
  return (
    <a
      aria-current={active ? 'page' : undefined}
      aria-label={label}
      className={cn(
        'relative',
        navBtnBase,
        active ? navBtnActive : navBtnInactive
      )}
      href={href}
      title={label}
    >
      <Icon className="h-5 w-5" />
      {badge != null && badge > 0 && (
        <span className="absolute -top-0.5 -right-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-[var(--color-status-danger)] px-0.5 text-[9px] leading-none font-bold text-white">
          {badge > 99 ? '99+' : badge}
        </span>
      )}
    </a>
  )
}
