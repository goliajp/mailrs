import { useQuery } from '@tanstack/react-query'

import { Attention } from '@/components/admin-overview/attention'
import { attentionItems } from '@/components/admin-overview/flow-model'
import { FlowStrip } from '@/components/admin-overview/flow-strip'
import { uptime } from '@/components/admin-overview/format'
import { RecentAudit } from '@/components/admin-overview/recent-audit'
import { AdminErrorState, AdminPageShell } from '@/components/admin-page'
import { useT } from '@/i18n'
import { adminKeys } from '@/lib/query-keys'
import { adminObjectGet } from '@/wire/endpoints/admin'
import { wireAdminFlow } from '@/wire/endpoints/admin-flow'

type Health = { kevy?: boolean | null; uptime_secs?: number; version?: string }

const REFRESH_MS = 10_000

/** Where mail is in the server right now, and what needs a person. */
export function AdminOverview() {
  const t = useT()
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
    <AdminPageShell subtitle={subtitleOf(health.data ?? null, t)} title={t('admin.overview.title')}>
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
  const t = useT()
  if (error) return <AdminErrorState error={error} onRetry={onRetry} />
  if (flow === null)
    return (
      <div aria-busy className="bg-surface border-border h-40 animate-pulse rounded-lg border" />
    )
  return (
    <div className="space-y-6">
      <FlowStrip flow={flow} />
      <Attention items={attentionItems(flow, kevyState(health), t)} />
      <RecentAudit />
    </div>
  )
}

function kevyState(health: Health | null): boolean | null {
  if (health === null || typeof health.kevy !== 'boolean') return null
  return health.kevy
}

function subtitleOf(health: Health | null, t: ReturnType<typeof useT>): string | undefined {
  if (health === null || health.version === undefined) return undefined
  if (health.uptime_secs === undefined)
    return t('admin.overview.version', { version: health.version })
  return t('admin.overview.versionUp', {
    uptime: uptime(health.uptime_secs, t),
    version: health.version,
  })
}
