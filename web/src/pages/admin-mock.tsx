import type { MockAdminOverview } from '@/wire/schemas/mock'

import { Button } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'
import { FlaskConical } from 'lucide-react'
import { useNavigate } from 'react-router'

import { bytes } from '@/components/admin-overview/flow-model'
import {
  AdminEmptyState,
  AdminErrorState,
  AdminPageShell,
  AdminSection,
} from '@/components/admin-page'
import { MockSettingsForm } from '@/components/mock/settings-form'
import { adminKeys } from '@/lib/query-keys'
import { wireAdminMockOverview } from '@/wire/endpoints/mock'

/** How much mock mail the server holds, and the rules it keeps it by. */
export function AdminMock() {
  const navigate = useNavigate()
  const overview = useQuery({
    queryKey: adminKeys.mock(),
    refetchInterval: 30_000,
    queryFn: ({ signal }) => wireAdminMockOverview(signal),
  })
  return (
    <AdminPageShell
      actions={
        <Button onClick={() => navigate('/mock?scope=all')} size="sm" variant="secondary">
          Open namespaces
        </Button>
      }
      subtitle="Usage and settings for disposable test mailboxes. To create or read namespaces, use Open namespaces"
      title="Mock mail"
    >
      <Body
        data={overview.data ?? null}
        error={overview.error}
        onRetry={() => void overview.refetch()}
      />
    </AdminPageShell>
  )
}

function Body({
  data,
  error,
  onRetry,
}: {
  data: MockAdminOverview | null
  error: Error | null
  onRetry: () => void
}) {
  if (error) {
    return <AdminErrorState error={error} onRetry={onRetry} what="mock mail usage and settings" />
  }
  if (data === null) return <div aria-busy className="bg-surface h-40 animate-pulse rounded-lg" />
  if (data.domain === '') {
    return (
      <AdminEmptyState
        description="Set MAILRS_MOCK_DOMAIN on the receiver, fastcore, webapi and sender, and point that domain's MX at this server"
        icon={<FlaskConical className="h-10 w-10" />}
        title="No mock domain yet"
      />
    )
  }
  return (
    <div className="space-y-6">
      <AdminSection title="Usage">
        <Usage data={data} />
      </AdminSection>
      <AdminSection title="Settings">
        <div className="px-4 py-4">
          <MockSettingsForm saved={data.settings} />
        </div>
      </AdminSection>
    </div>
  )
}

function Figure({ detail, label, value }: { detail?: string; label: string; value: string }) {
  return (
    <div>
      <dt className="text-fg-muted text-xs">{label}</dt>
      <dd className="text-fg text-lg font-semibold tabular-nums">{value}</dd>
      {detail && <dd className="text-fg-muted text-xs">{detail}</dd>}
    </div>
  )
}

function meterTone(share: number): string {
  if (share >= 0.9) return 'bg-danger'
  if (share >= 0.7) return 'bg-warning'
  return 'bg-accent'
}

function Usage({ data }: { data: MockAdminOverview }) {
  const max = data.settings.max_total_mb * 1024 * 1024
  const share = Math.min(1, data.usage.bytes / max)
  const u = data.usage
  return (
    <div className="space-y-4 px-4 py-4">
      <div>
        <div className="flex items-baseline justify-between text-sm">
          <span className="text-fg">
            Storage on <span className="font-mono">{data.domain}</span>
          </span>
          <span className="text-fg-secondary tabular-nums">
            {bytes(u.bytes)} of {bytes(max)}
          </span>
        </div>
        <div
          aria-label="Storage used"
          aria-valuemax={100}
          aria-valuemin={0}
          aria-valuenow={Math.round(share * 100)}
          className="bg-border mt-2 h-2 overflow-hidden rounded-full"
          role="meter"
        >
          <div className={`h-full ${meterTone(share)}`} style={{ width: `${share * 100}%` }} />
        </div>
      </div>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-sm sm:grid-cols-4">
        <Figure detail={`${u.owners} owners`} label="Namespaces" value={`${u.namespaces}`} />
        <Figure
          detail="Received and captured outgoing"
          label="Messages stored"
          value={`${u.received}`}
        />
        <Figure label="Dropped at the per-namespace limit" value={`${u.dropped}`} />
        <Figure label="Refused, no such namespace" value={`${u.rejected_unknown_ns}`} />
      </dl>
    </div>
  )
}
