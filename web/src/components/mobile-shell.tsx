import { Home, Inbox, Server, Settings } from 'lucide-react'
import { Link, useLocation } from 'react-router'

import { useCurrentUnreadCount } from '@/hooks/use-current-mail-filters'

// independent mobile app shell — no AppShell/Pane dependency
// fixed height viewport with bottom navigation
export function MobileShell({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-col" style={{ height: '100dvh' }}>
      <main className="min-h-0 flex-1 overflow-hidden">{children}</main>
      <MobileNav />
    </div>
  )
}

const NAV_ITEMS = [
  { href: '/', icon: Home, label: 'Home' },
  { href: '/mail', icon: Inbox, label: 'Mail' },
  { href: '/admin', icon: Server, label: 'Admin' },
  { href: '/settings', icon: Settings, label: 'Settings' },
] as const

function MobileNav() {
  const { pathname } = useLocation()
  const unreadCount = useCurrentUnreadCount()

  const activeSection = pathname.startsWith('/admin')
    ? '/admin'
    : pathname.startsWith('/settings')
      ? '/settings'
      : pathname.startsWith('/mail')
        ? '/mail'
        : '/'

  return (
    <nav
      className="border-border bg-surface flex shrink-0 items-stretch border-t"
      style={{ paddingBottom: 'var(--safe-area-bottom)' }}
    >
      {NAV_ITEMS.map((item) => {
        const active = activeSection === item.href
        return (
          <Link
            aria-current={active ? 'page' : undefined}
            className={`text-mini relative flex flex-1 flex-col items-center gap-0.5 py-2 transition-colors ${
              active ? 'text-accent' : 'text-fg-muted'
            }`}
            key={item.href}
            to={item.href}
          >
            <item.icon className="h-5 w-5" />
            <span>{item.label}</span>
            {item.href === '/mail' && unreadCount > 0 && (
              <span className="bg-danger text-tiny absolute top-1 left-1/2 ml-2 grid h-4 min-w-4 place-items-center rounded-full px-0.5 leading-none font-bold text-white">
                {unreadCount > 99 ? '99+' : unreadCount}
              </span>
            )}
          </Link>
        )
      })}
    </nav>
  )
}
