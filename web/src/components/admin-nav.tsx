import type { Key } from '@/i18n'

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
export type NavItem = { icon: React.ReactNode; label: Key; to: string }

// grouped by what an operator comes to do: watch mail move, manage who
// and what the server hosts, look back at what happened, change settings
export const navGroups: { items: NavItem[]; label: Key }[] = [
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

/** Every admin page the sidebar offers, for the test that each one has a permission. */
export const ADMIN_PAGES = navGroups.flatMap((g) => g.items.map((i) => i.to))
