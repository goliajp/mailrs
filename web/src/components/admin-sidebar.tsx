import {
  ArrowLeftRight,
  Blocks,
  Eye,
  Globe,
  LayoutGrid,
  ListOrdered,
  Mail,
  ScrollText,
  Shield,
  Users,
} from 'lucide-react'
import { NavLink } from 'react-router'

const navItems = [
  {
    icon: <LayoutGrid className="h-4 w-4" />,
    label: 'Overview',
    to: '/admin/overview',
  },
  {
    icon: <Globe className="h-4 w-4" />,
    label: 'Domains',
    to: '/admin/domains',
  },
  {
    icon: <Users className="h-4 w-4" />,
    label: 'Accounts',
    to: '/admin/accounts',
  },
  {
    icon: <ArrowLeftRight className="h-4 w-4" />,
    label: 'Aliases',
    to: '/admin/aliases',
  },
  {
    icon: <Shield className="h-4 w-4" />,
    label: 'Groups',
    to: '/admin/groups',
  },
  {
    icon: <Mail className="h-4 w-4" />,
    label: 'Email Groups',
    to: '/admin/email-groups',
  },
  {
    icon: <Blocks className="h-4 w-4" />,
    label: 'Apps',
    to: '/admin/apps',
  },
  {
    icon: <ListOrdered className="h-4 w-4" />,
    label: 'Queues',
    to: '/admin/queues',
  },
  {
    icon: <Eye className="h-4 w-4" />,
    label: 'Mail Audit',
    to: '/admin/mail-audit',
  },
  {
    icon: <ScrollText className="h-4 w-4" />,
    label: 'Audit Log',
    to: '/admin/audit-log',
  },
]

export function AdminSidebar() {
  return (
    <>
      {/* mobile: horizontal tab bar */}
      <nav className="border-border flex items-center gap-1.5 overflow-x-auto border-b px-3 py-2 select-none md:hidden">
        {navItems.map((item) => (
          <NavLink
            className={({ isActive }) =>
              `flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs transition-colors ${
                isActive
                  ? 'bg-accent/10 text-fg font-medium'
                  : 'text-fg-muted hover:bg-bg-secondary'
              }`
            }
            key={item.to}
            to={item.to}
          >
            <span className="text-fg-muted">{item.icon}</span>
            <span>{item.label}</span>
          </NavLink>
        ))}
      </nav>

      {/* desktop: vertical sidebar */}
      <aside className="border-border hidden h-full w-48 shrink-0 flex-col border-r select-none md:flex">
        <div className="px-3 py-4">
          <p className="text-fg-muted text-xs font-medium tracking-wider uppercase">
            Server
          </p>
        </div>
        <nav className="flex-1 px-3">
          {navItems.map((item) => (
            <NavLink
              className={({ isActive }) =>
                `flex w-full items-center gap-2.5 rounded-md px-3 py-1.5 text-sm transition-colors ${
                  isActive
                    ? 'bg-accent/10 text-fg font-medium'
                    : 'text-fg-secondary hover:bg-bg-secondary'
                }`
              }
              key={item.to}
              to={item.to}
            >
              <span className="text-fg-muted">{item.icon}</span>
              <span>{item.label}</span>
            </NavLink>
          ))}
        </nav>
      </aside>
    </>
  )
}
