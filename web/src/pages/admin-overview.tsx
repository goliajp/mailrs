import { useQuery } from '@tanstack/react-query'

import { Attention } from '@/components/admin-overview/attention'
import { attentionItems } from '@/components/admin-overview/flow-model'
import { FlowStrip } from '@/components/admin-overview/flow-strip'
import { uptime } from '@/components/admin-overview/format'
import { RecentAudit } from '@/components/admin-overview/recent-audit'
import { AdminErrorState, AdminPageShell } from '@/components/admin-page'
import { adminKeys } from '@/lib/query-keys'
import { adminObjectGet } from '@/wire/endpoints/admin'
import { wireAdminFlow } from '@/wire/endpoints/admin-flow'

type Health = { kevy?: boolean | null; uptime_secs?: number; version?: string }

const REFRESH_MS = 10_000

/** Where mail is in the server right now, and what needs a person. */
export function AdminOverview() {
  const flow = useQuery({
    queryKey: adminKeys.overviewFlow(),
    refetchInterval: REFRESH_MS,
    queryFn: ({ signal }) => wireAdminFlow(signal),
  })
  const health = useQuery({
    queryKey: adminKeys.overviewHealth(),
    refetchInterval: REFRESH_MS,
    queryFn: ({ signal }) => adminObjectGet<Health>('/health', signal),
  })

  return (
    <AdminPageShell subtitle={subtitleOf(health.data ?? null)} title="Overview">
      <Body
        error={flow.error}
        flow={flow.data ?? null}
        health={health.data ?? null}
        onRetry={() => void flow.refetch()}
      />
    </AdminPageShell>
  )
}

function Body({
  error,
  flow,
  health,
  onRetry,
}: {
  error: Error | null
  flow: Awaited<ReturnType<typeof wireAdminFlow>> | null
  health: Health | null
  onRetry: () => void
}) {
  if (error) return <AdminErrorState error={error} onRetry={onRetry} />
  if (flow === null)
    return (
      <div aria-busy className="bg-surface border-border h-40 animate-pulse rounded-lg border" />
    )
  return (
    <div className="space-y-6">
      <FlowStrip flow={flow} />
      <Attention items={attentionItems(flow, kevyState(health))} />
      <RecentAudit />
    </div>
  )
}

function kevyState(health: Health | null): boolean | null {
  if (health === null || typeof health.kevy !== 'boolean') return null
  return health.kevy
}

function subtitleOf(health: Health | null): string | undefined {
  if (health === null || health.version === undefined) return undefined
  if (health.uptime_secs === undefined) return `Web API ${health.version}`
  return `Web API ${health.version}, up ${uptime(health.uptime_secs)}`
}
