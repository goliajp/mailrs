import { ArrowLeftRight, Blocks, Globe, LayoutGrid, ListOrdered, Mail, ScrollText, Shield, Users } from 'lucide-react'
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
    to: '/admin/groups',
    label: 'Groups',
    icon: <Shield className="h-4 w-4" />,
  },
  {
    to: '/admin/email-groups',
    label: 'Email Groups',
    icon: <Mail className="h-4 w-4" />,
  },
  {
    to: '/admin/apps',
    label: 'Apps',
    icon: <Blocks className="h-4 w-4" />,
  },
  {
    to: '/admin/queues',
    label: 'Queues',
    icon: <ListOrdered className="h-4 w-4" />,
  },
  {
    to: '/admin/audit-log',
    label: 'Audit Log',
    icon: <ScrollText className="h-4 w-4" />,
  },
]

export function AdminSidebar() {
  return (
    <>
      {/* mobile: horizontal tab bar */}
      <nav className="flex select-none items-center gap-1 overflow-x-auto border-b border-[var(--color-border-default)] px-2 py-1.5 md:hidden">
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
      <aside className="hidden h-full w-48 shrink-0 select-none flex-col border-r border-[var(--color-border-default)] md:flex">
        <div className="px-3 py-4">
          <p className="text-xs font-medium uppercase tracking-wider text-[var(--color-text-tertiary)]">Server</p>
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
      </aside>
    </>
  )
}
