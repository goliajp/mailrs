import type { WirePolicy, WireUsage } from '@/wire/schemas/protocol-logins'

import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { KeyRound } from 'lucide-react'

import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminTable,
  AdminTableSkeleton,
  type Column,
} from '@/components/admin-page'
import { type Key, useT } from '@/i18n'
import { formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { wireProtocolLogins, wireSetProtocolPassword } from '@/wire/endpoints/protocol-logins'

const HEADERS: Key[] = [
  'admin.signIns.account',
  'admin.protocol.protocol',
  'admin.protocol.accountPassword',
  'admin.protocol.appPassword',
  'admin.protocol.setting',
]
const POLICIES: WirePolicy[] = ['inherit', 'allow', 'app-only']

/**
 * Successful sign-ins to the mail protocols over the last 30 days, with
 * the account password and with app passwords, and each account's setting
 * for whether its own password still works there. An account is moved to
 * app passwords only after its owner has been told and has had time to
 * make one.
 */
export function AdminProtocolLogins() {
  const t = useT()
  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: [...adminKeys.all(), 'protocol-logins'],
    queryFn: ({ signal }) => wireProtocolLogins(signal),
  })
  const reload = () => void refetch()
  const set = async (account: string, policy: WirePolicy) => {
    try {
      await wireSetProtocolPassword(account, policy)
      toast.success(`${account}: ${t(policyLabel(policy))}`)
    } catch {
      toast.error(t('admin.protocol.changeFailed'))
    } finally {
      reload()
    }
  }
  const columns: Column<WireUsage>[] = [
    {
      header: t('admin.signIns.account'),
      key: 'account',
      cell: (r) => <span className="text-xs">{r.account}</span>,
    },
    {
      header: t('admin.protocol.protocol'),
      key: 'channel',
      cell: (r) => <span className="font-mono text-xs">{r.channel}</span>,
    },
    {
      header: t('admin.protocol.accountPassword'),
      key: 'password',
      cell: (r) => <span className="text-xs">{used(r.password_logins, r.password_last, t)}</span>,
    },
    {
      header: t('admin.protocol.appPassword'),
      key: 'app',
      cell: (r) => (
        <span className="text-xs">{used(r.app_password_logins, r.app_password_last, t)}</span>
      ),
    },
    {
      header: t('admin.protocol.setting'),
      key: 'policy',
      cell: (r) => (
        <select
          aria-label={t('admin.protocol.settingFor', { account: r.account })}
          className="border-border bg-bg rounded border px-1 text-xs"
          onChange={(e) => void set(r.account, e.target.value as WirePolicy)}
          value={data?.policies[r.account] ?? 'inherit'}
        >
          {POLICIES.map((p) => (
            <option key={p} value={p}>
              {t(policyLabel(p))}
            </option>
          ))}
        </select>
      ),
    },
  ]
  let body = <AdminTableSkeleton cols={5} headers={HEADERS.map((h) => t(h))} rows={4} />
  if (error) {
    body = (
      <AdminErrorState
        error={error}
        onRetry={reload}
        retryDisabled={isFetching}
        what={t('admin.protocol.listName')}
      />
    )
  } else if (!isPending && (data?.items.length ?? 0) === 0) {
    body = (
      <AdminEmptyState
        description={t('admin.protocol.emptyHelp')}
        icon={<KeyRound className="h-10 w-10" />}
        title={t('admin.protocol.empty')}
      />
    )
  } else if (data) {
    body = (
      <AdminTable
        columns={columns}
        label={t('admin.protocol.listName')}
        rowKey={(r) => `${r.account} ${r.channel}`}
        rows={data.items}
      />
    )
  }
  return (
    <AdminPageShell
      subtitle={t('admin.protocol.subtitle', { policy: defaultPolicy(data?.default, t) })}
      title={t('admin.protocol.title')}
    >
      {body}
    </AdminPageShell>
  )
}

function defaultPolicy(policy: undefined | WirePolicy, t: ReturnType<typeof useT>): string {
  if (policy === undefined) return '…'
  return t(policyLabel(policy))
}

function policyLabel(policy: WirePolicy): Key {
  if (policy === 'allow') return 'admin.protocol.policy.allow'
  if (policy === 'app-only') return 'admin.protocol.policy.appOnly'
  return 'admin.protocol.policy.inherit'
}

function used(n: number, last: null | number, t: ReturnType<typeof useT>): string {
  if (n === 0 || last === null) return '—'
  return t('admin.protocol.used', { last: formatFullDate(last), n })
}
