import {
  ArrowLeftRight,
  Eye,
  Filter,
  FlaskConical,
  Globe,
  KeyRound,
  LayoutGrid,
  ListOrdered,
  LockKeyhole,
  Mail,
  ScrollText,
  Settings,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Users,
} from 'lucide-react'
import { NavLink } from 'react-router'

import { type Key, useT } from '@/i18n'

type NavItem = { icon: React.ReactNode; label: Key; to: string }

// grouped by what an operator comes to do: watch mail move, manage who
// and what the server hosts, look back at what happened, change settings
const navGroups: { items: NavItem[]; label: Key }[] = [
  {
    items: [
      {
        icon: <LayoutGrid className="h-4 w-4" />,
        label: 'admin.nav.overview',
        to: '/admin/overview',
      },
      { icon: <ListOrdered className="h-4 w-4" />, label: 'admin.nav.queues', to: '/admin/queues' },
      { icon: <ShieldCheck className="h-4 w-4" />, label: 'admin.nav.dmarc', to: '/admin/dmarc' },
      {
        icon: <Filter className="h-4 w-4" />,
        label: 'admin.nav.senderLists',
        to: '/admin/greylist',
      },
      { icon: <FlaskConical className="h-4 w-4" />, label: 'admin.nav.mock', to: '/admin/mock' },
    ],
    label: 'admin.nav.groupFlow',
  },
  {
    items: [
      { icon: <Globe className="h-4 w-4" />, label: 'admin.nav.domains', to: '/admin/domains' },
      { icon: <Users className="h-4 w-4" />, label: 'admin.nav.accounts', to: '/admin/accounts' },
      {
        icon: <LockKeyhole className="h-4 w-4" />,
        label: 'admin.nav.clearPasswords',
        to: '/admin/password-clear',
      },
      {
        icon: <ArrowLeftRight className="h-4 w-4" />,
        label: 'admin.nav.aliases',
        to: '/admin/aliases',
      },
      {
        icon: <Mail className="h-4 w-4" />,
        label: 'admin.nav.emailGroups',
        to: '/admin/email-groups',
      },
      {
        icon: <Shield className="h-4 w-4" />,
        label: 'admin.nav.permissionGroups',
        to: '/admin/groups',
      },
    ],
    label: 'admin.nav.groupDirectory',
  },
  {
    items: [
      { icon: <Eye className="h-4 w-4" />, label: 'admin.nav.mailAudit', to: '/admin/mail-audit' },
      {
        icon: <ScrollText className="h-4 w-4" />,
        label: 'admin.nav.auditLog',
        to: '/admin/audit-log',
      },
      { icon: <KeyRound className="h-4 w-4" />, label: 'admin.nav.signIns', to: '/admin/sign-ins' },
      {
        icon: <ShieldAlert className="h-4 w-4" />,
        label: 'admin.nav.screening',
        to: '/admin/screening',
      },
      {
        icon: <KeyRound className="h-4 w-4" />,
        label: 'admin.nav.protocolSignIns',
        to: '/admin/protocol-sign-ins',
      },
    ],
    label: 'admin.nav.groupOversight',
  },
  {
    items: [
      {
        icon: <Settings className="h-4 w-4" />,
        label: 'admin.nav.systemConfig',
        to: '/admin/system-config',
      },
    ],
    label: 'admin.nav.groupSettings',
  },
]

const allItems = navGroups.flatMap((g) => g.items)

export function AdminSidebar() {
  const t = useT()
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
        {navGroups.map((group) => (
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
