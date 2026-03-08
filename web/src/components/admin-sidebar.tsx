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
      <nav className="flex items-center gap-1 overflow-x-auto border-b border-zinc-200 bg-zinc-50 px-2 py-1.5 md:hidden dark:border-zinc-800 dark:bg-zinc-900/50">
        <NavLink
          to="/"
          className="shrink-0 rounded-md px-2 py-1 text-xs text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300"
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
                  ? 'bg-zinc-200 font-medium text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
                  : 'text-zinc-500 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800/50'
              }`
            }
          >
            <span className="text-zinc-400 dark:text-zinc-500">{item.icon}</span>
            <span>{item.label}</span>
          </NavLink>
        ))}
      </nav>

      {/* desktop: vertical sidebar */}
      <aside className="hidden h-full w-56 shrink-0 flex-col border-r border-zinc-200 bg-zinc-50 md:flex dark:border-zinc-800 dark:bg-zinc-900/50">
        <div className="p-4">
          <h1 className="text-lg font-semibold tracking-tight text-zinc-900 dark:text-zinc-100">
            mailrs
          </h1>
          <p className="text-xs text-zinc-500 dark:text-zinc-400">Admin</p>
        </div>
        <nav className="flex-1 px-2">
          {navItems.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) =>
                `flex w-full items-center gap-2.5 rounded-md px-3 py-1.5 text-sm transition-colors ${
                  isActive
                    ? 'bg-zinc-200 font-medium text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
                    : 'text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800/50'
                }`
              }
            >
              <span className="text-zinc-500 dark:text-zinc-400">
                {item.icon}
              </span>
              <span>{item.label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="space-y-1 border-t border-zinc-200 p-3 dark:border-zinc-800">
          <NavLink
            to="/protocol"
            className="block text-xs text-zinc-400 transition-colors hover:text-zinc-600 dark:hover:text-zinc-300"
          >
            SMTP Monitor
          </NavLink>
          <NavLink
            to="/"
            className="block text-xs text-zinc-400 transition-colors hover:text-zinc-600 dark:hover:text-zinc-300"
          >
            Back to Mail
          </NavLink>
        </div>
      </aside>
    </>
  )
}
