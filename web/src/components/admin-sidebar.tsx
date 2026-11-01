import { useAtomValue } from 'jotai'
import { NavLink } from 'react-router'

import { useT } from '@/i18n'
import { canOpenAdminPage } from '@/lib/admin-access'
import { authAtom } from '@/store/auth'

import { navGroups } from './admin-nav'

export function AdminSidebar() {
  const t = useT()
  const who = useAtomValue(authAtom)
  const groups = navGroups
    .map((g) => ({ ...g, items: g.items.filter((i) => canOpenAdminPage(who, i.to)) }))
    .filter((g) => g.items.length > 0)
  const allItems = groups.flatMap((g) => g.items)
  return (
    <>
      {/* mobile: one scrolling row of every page */}
      <nav className="border-border flex items-center gap-1.5 overflow-x-auto border-b px-3 py-2 select-none md:hidden">
        {allItems.map((item) => (
          <NavLink
            className={({ isActive }) => linkClass(isActive, true)}
            key={item.to}
            to={item.to}
          >
            <span className="text-fg-muted">{item.icon}</span>
            <span>{t(item.label)}</span>
          </NavLink>
        ))}
      </nav>

      {/* desktop: grouped column */}
      <aside className="border-border hidden h-full w-52 shrink-0 flex-col overflow-y-auto border-r py-3 select-none md:flex">
        {groups.map((group) => (
          <nav aria-label={t(group.label)} className="mb-4 px-3" key={group.label}>
            <p className="text-fg-muted mb-1 px-3 text-xs">{t(group.label)}</p>
            {group.items.map((item) => (
              <NavLink
                className={({ isActive }) => linkClass(isActive, false)}
                key={item.to}
                to={item.to}
              >
                <span className="text-fg-muted">{item.icon}</span>
                <span>{t(item.label)}</span>
              </NavLink>
            ))}
          </nav>
        ))}
      </aside>
    </>
  )
}

function linkClass(isActive: boolean, compact: boolean): string {
  const base = compact
    ? 'flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs transition-colors'
    : 'flex w-full items-center gap-2.5 rounded-md px-3 py-1.5 text-sm transition-colors'
  if (isActive) return `${base} bg-accent/10 text-fg font-medium`
  if (compact) return `${base} text-fg-muted hover:bg-bg-secondary`
  return `${base} text-fg-secondary hover:bg-bg-secondary`
}
