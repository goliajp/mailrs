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
import { formatFullDate } from '@/lib/format'
import { adminKeys } from '@/lib/query-keys'
import { wireProtocolLogins, wireSetProtocolPassword } from '@/wire/endpoints/protocol-logins'

const HEADERS = ['Account', 'Protocol', 'Account password', 'App password', 'Setting']
const POLICIES: WirePolicy[] = ['inherit', 'allow', 'app-only']

/**
 * Successful sign-ins to the mail protocols over the last 30 days, with
 * the account password and with app passwords, and each account's setting
 * for whether its own password still works there. An account is moved to
 * app passwords only after its owner has been told and has had time to
 * make one.
 */
export function AdminProtocolLogins() {
  const { data, error, isFetching, isPending, refetch } = useQuery({
    queryKey: [...adminKeys.all(), 'protocol-logins'],
    queryFn: ({ signal }) => wireProtocolLogins(signal),
  })
  const reload = () => void refetch()
  const set = async (account: string, policy: WirePolicy) => {
    try {
      await wireSetProtocolPassword(account, policy)
      toast.success(`${account}: ${policy}`)
    } catch {
      toast.error('Could not change the setting')
    } finally {
      reload()
    }
  }
  const columns: Column<WireUsage>[] = [
    {
      header: 'Account',
      key: 'account',
      cell: (r) => <span className="text-xs">{r.account}</span>,
    },
    {
      header: 'Protocol',
      key: 'channel',
      cell: (r) => <span className="font-mono text-xs">{r.channel}</span>,
    },
    {
      header: 'Account password',
      key: 'password',
      cell: (r) => <span className="text-xs">{used(r.password_logins, r.password_last)}</span>,
    },
    {
      header: 'App password',
      key: 'app',
      cell: (r) => (
        <span className="text-xs">{used(r.app_password_logins, r.app_password_last)}</span>
      ),
    },
    {
      header: 'Setting',
      key: 'policy',
      cell: (r) => (
        <select
          aria-label={`Setting for ${r.account}`}
          className="border-border bg-bg rounded border px-1 text-xs"
          onChange={(e) => void set(r.account, e.target.value as WirePolicy)}
          value={data?.policies[r.account] ?? 'inherit'}
        >
          {POLICIES.map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
      ),
    },
  ]
  let body = <AdminTableSkeleton cols={5} headers={HEADERS} rows={4} />
  if (error) {
    body = (
      <AdminErrorState
        error={error}
        onRetry={reload}
        retryDisabled={isFetching}
        what="protocol sign-ins"
      />
    )
  } else if (!isPending && (data?.items.length ?? 0) === 0) {
    body = (
      <AdminEmptyState
        description="No mail app has signed in for 30 days"
        icon={<KeyRound className="h-10 w-10" />}
        title="No protocol sign-ins"
      />
    )
  } else if (data) {
    body = (
      <AdminTable
        columns={columns}
        label="protocol sign-ins"
        rowKey={(r) => `${r.account} ${r.channel}`}
        rows={data.items}
      />
    )
  }
  return (
    <AdminPageShell
      subtitle={`Mail apps that signed in over the last 30 days. The default for accounts set to inherit is ${data?.default ?? '…'}.`}
      title="Protocol sign-ins"
    >
      {body}
    </AdminPageShell>
  )
}

function used(n: number, last: null | number): string {
  if (n === 0 || last === null) return '—'
  return `${n} · last ${formatFullDate(last)}`
}
