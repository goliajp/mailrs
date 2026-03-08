import { ArrowLeftRight, Globe, LayoutGrid, ListOrdered, Users } from 'lucide-react'
import { NavLink } from 'react-router'

const navItems = [
  {
    to: '/admin/overview',
    label: 'Overview',
    icon: <LayoutGrid className="h-4 w-4" />,
  },
  {
    to: '/admin/domains',
    label: 'Domains',
    icon: <Globe className="h-4 w-4" />,
  },
  {
    to: '/admin/accounts',
    label: 'Accounts',
    icon: <Users className="h-4 w-4" />,
  },
  {
    to: '/admin/aliases',
    label: 'Aliases',
    icon: <ArrowLeftRight className="h-4 w-4" />,
  },
  {
    to: '/admin/queues',
    label: 'Queues',
    icon: <ListOrdered className="h-4 w-4" />,
  },
]

export function AdminSidebar() {
  return (
    <>
      {/* mobile: horizontal tab bar */}
      <nav className="flex select-none items-center gap-1 overflow-x-auto border-b border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] px-2 py-1.5 md:hidden">
        <NavLink
          to="/"
          className="shrink-0 rounded-md px-2 py-1 text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]"
        >
          Mail
        </NavLink>
        {navItems.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            className={({ isActive }) =>
              `flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1 text-xs transition-colors ${
                isActive
                  ? 'bg-[var(--color-bg-selected)] font-medium text-[var(--color-text-primary)]'
                  : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)]'
              }`
            }
          >
            <span className="text-[var(--color-text-tertiary)]">{item.icon}</span>
            <span>{item.label}</span>
          </NavLink>
        ))}
      </nav>

      {/* desktop: vertical sidebar */}
      <aside className="hidden h-full w-56 shrink-0 select-none flex-col border-r border-[var(--color-border-default)] bg-[var(--color-bg-sunken)] md:flex">
        <div className="p-4">
          <h1 className="text-lg font-semibold tracking-tight text-[var(--color-text-primary)]">
            mailrs
          </h1>
          <p className="text-xs text-[var(--color-text-tertiary)]">Admin</p>
        </div>
        <nav className="flex-1 px-2">
          {navItems.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) =>
                `flex w-full items-center gap-2.5 rounded-md px-3 py-1.5 text-sm transition-colors ${
                  isActive
                    ? 'bg-[var(--color-bg-selected)] font-medium text-[var(--color-text-primary)]'
                    : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-hover)]'
                }`
              }
            >
              <span className="text-[var(--color-text-tertiary)]">
                {item.icon}
              </span>
              <span>{item.label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="space-y-1 border-t border-[var(--color-border-default)] p-3">
          <NavLink
            to="/protocol"
            className="block text-xs text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-secondary)]"
          >
            SMTP Monitor
          </NavLink>
          <NavLink
            to="/"
            className="block text-xs text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-secondary)]"
          >
            Back to Mail
          </NavLink>
        </div>
      </aside>
    </>
  )
}
