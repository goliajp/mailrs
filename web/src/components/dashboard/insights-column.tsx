import type { CategoryCount, ConversationSummary } from '@/lib/types'

import { AlertTriangle, Mail, ShieldAlert, TrendingUp, Users } from 'lucide-react'

import { SenderAvatar } from '@/components/sender-avatar'

import { formatBytes } from './_shared'
import { CategoryBar } from './category-bar'
import { Section } from './section'

type FolderInfo = { name: string; total: number; unseen: number }
type InsightsColumnProps = {
  categoryData: CategoryCount[]
  folders: FolderInfo[]
  onOpenThread: (threadId: string) => void
  securityAlerts: ConversationSummary[]
  storageBytes: number
  topSenders: TopSender[]
  totalCategorized: number
  totalMessages: number
}

type TopSender = { count: number; email: string; name: string }

export function InsightsColumn({
  categoryData,
  folders,
  onOpenThread,
  securityAlerts,
  storageBytes,
  topSenders,
  totalCategorized,
  totalMessages,
}: InsightsColumnProps) {
  return (
    <div className="space-y-6">
      {securityAlerts.length > 0 && (
        <Section icon={ShieldAlert} title="Security Alerts">
          <div className="space-y-0.5">
            {securityAlerts.map((c) => (
              <button
                className="hover:bg-bg-secondary flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left transition-colors"
                key={c.thread_id}
                onClick={() => onOpenThread(c.thread_id)}
                type="button"
              >
                <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-red-500/10">
                  <AlertTriangle className="h-3.5 w-3.5 text-red-500" />
                </div>
                <div className="min-w-0 flex-1">
                  <p className="text-fg truncate text-sm font-medium">
                    {c.subject || '(no subject)'}
                  </p>
                  <p className="text-danger truncate text-xs">{c.category}</p>
                </div>
              </button>
            ))}
          </div>
        </Section>
      )}

      {categoryData.length > 0 && (
        <Section icon={TrendingUp} title="Categories">
          <div className="space-y-2.5">
            {categoryData.map((cat) => (
              <CategoryBar
                category={cat.category}
                count={cat.count}
                key={cat.category}
                total={totalCategorized}
              />
            ))}
          </div>
        </Section>
      )}

      {topSenders.length > 0 && (
        <Section icon={Users} title="Top Contacts">
          <div className="space-y-0.5">
            {topSenders.map((s) => (
              <div
                className="hover:bg-bg-secondary flex items-center gap-2.5 rounded-md px-2 py-1.5 transition-colors"
                key={s.email}
              >
                <SenderAvatar sender={`${s.name} <${s.email}>`} size={28} />
                <div className="min-w-0 flex-1">
                  <p className="text-fg truncate text-sm font-medium">{s.name}</p>
                  <p className="text-fg-muted truncate text-xs">{s.email}</p>
                </div>
                <span className="bg-bg-secondary text-fg-muted shrink-0 rounded-full px-1.5 py-0.5 text-xs tabular-nums md:text-[10px]">
                  {s.count}
                </span>
              </div>
            ))}
          </div>
        </Section>
      )}

      {totalMessages > 0 && (
        <Section icon={Mail} title="Mailbox">
          <div className="space-y-2 px-2 text-sm">
            <div className="flex items-center justify-between">
              <span className="text-fg-muted">Total emails</span>
              <span className="text-fg font-medium tabular-nums">
                {totalMessages.toLocaleString()}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-fg-muted">Storage</span>
              <span className="text-fg font-medium tabular-nums">{formatBytes(storageBytes)}</span>
            </div>
          </div>
        </Section>
      )}

      {folders.length > 0 && (
        <Section icon={Mail} title="Folders">
          <div className="space-y-0.5">
            {folders
              .filter((f) => f.total > 0)
              .slice(0, 8)
              .map((f) => (
                <div
                  className="hover:bg-bg-secondary flex items-center justify-between rounded-md px-2 py-1.5 text-sm transition-colors"
                  key={f.name}
                >
                  <span className="text-fg-secondary">{f.name}</span>
                  <div className="flex items-center gap-2">
                    {f.unseen > 0 && (
                      <span className="bg-accent/10 text-accent rounded-full px-1.5 py-0.5 text-xs font-medium md:text-[10px]">
                        {f.unseen}
                      </span>
                    )}
                    <span className="text-fg-muted text-xs tabular-nums">{f.total}</span>
                  </div>
                </div>
              ))}
          </div>
        </Section>
      )}
    </div>
  )
}
