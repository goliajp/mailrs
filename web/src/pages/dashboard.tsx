import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef, useState } from 'react'
import {
  Mail, MailOpen, Star, AlertTriangle, Clock, Plus, RefreshCw,
  Shield, TrendingUp, Users, ChevronRight, Pin, Pen,
} from 'lucide-react'
import { useNavigate } from 'react-router'

import { Panel, PanelRow, Scroll } from '@/layouts/shell'
import { fetchJson } from '@/lib/api'
import type { ConversationSummary, CategoryCount } from '@/lib/types'
import { authAtom } from '@/store/auth'
import { selectedThreadIdAtom, composingNewAtom } from '@/store/chat'
import { SenderAvatar } from '@/components/sender-avatar'
import { CategoryBadge } from '@/components/category-badge'
import { cn } from '@/lib/cn'

type FolderInfo = { name: string; total: number; unseen: number }

type DashboardData = {
  conversations: ConversationSummary[]
  categories: CategoryCount[]
  folders: FolderInfo[]
}

const REFRESH_INTERVAL = 60_000

function useGreeting() {
  const hour = new Date().getHours()
  if (hour < 6) return 'Good night'
  if (hour < 12) return 'Good morning'
  if (hour < 18) return 'Good afternoon'
  return 'Good evening'
}

function formatRelative(ts: number): string {
  const now = Math.floor(Date.now() / 1000)
  const diff = now - ts
  if (diff < 60) return 'just now'
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
  if (diff < 604800) return `${Math.floor(diff / 86400)}d ago`
  return new Date(ts * 1000).toLocaleDateString('en', { month: 'short', day: 'numeric' })
}

function extractName(sender: string): string {
  const m = sender.match(/^"?([^"<]+)"?\s*</)
  return m ? m[1].trim() : sender.split('@')[0]
}

function extractEmail(sender: string): string {
  const m = sender.match(/<([^>]+)>/)
  return m ? m[1] : sender
}

function todayStart(): number {
  const d = new Date()
  d.setHours(0, 0, 0, 0)
  return Math.floor(d.getTime() / 1000)
}

export function Dashboard() {
  const auth = useAtomValue(authAtom)
  const greeting = useGreeting()
  const navigate = useNavigate()
  const setSelectedThread = useSetAtom(selectedThreadIdAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const [data, setData] = useState<DashboardData | null>(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const intervalRef = useRef<ReturnType<typeof setInterval>>(null)

  const load = useCallback(async (silent = false) => {
    if (!silent) setLoading(true)
    else setRefreshing(true)
    try {
      const [conversations, categories, folders] = await Promise.all([
        fetchJson<ConversationSummary[]>('/conversations?limit=200'),
        fetchJson<CategoryCount[]>('/conversations/categories'),
        fetchJson<FolderInfo[]>('/mail/folders'),
      ])
      setData({ conversations, categories, folders })
    } catch {
      // ignore
    } finally {
      setLoading(false)
      setRefreshing(false)
    }
  }, [])

  useEffect(() => {
    load()
    intervalRef.current = setInterval(() => load(true), REFRESH_INTERVAL)
    return () => { if (intervalRef.current) clearInterval(intervalRef.current) }
  }, [load])

  const goToThread = useCallback((threadId: string) => {
    setSelectedThread(threadId)
    navigate('/mail')
  }, [navigate, setSelectedThread])

  const handleCompose = useCallback(() => {
    setComposingNew(true)
    navigate('/mail')
  }, [navigate, setComposingNew])

  const inbox = data?.folders.find((f) => f.name === 'INBOX')
  const totalUnread = inbox?.unseen ?? 0
  const todayTs = todayStart()
  const todayCount = data?.conversations.filter((c) => c.last_date >= todayTs).length ?? 0
  const starredCount = data?.conversations.filter((c) => c.flagged).length ?? 0
  const importantCount = data?.conversations.filter((c) => c.importance_level === 'high' && c.unread_count > 0).length ?? 0

  // derive lists
  const pinned = data?.conversations.filter((c) => c.pinned).slice(0, 5) ?? []

  const needsAttention = data?.conversations
    .filter((c) => c.unread_count > 0 && (c.importance_level === 'high' || c.flagged))
    .filter((c) => !pinned.some((p) => p.thread_id === c.thread_id))
    .slice(0, 8) ?? []

  const recentUnread = data?.conversations
    .filter((c) => c.unread_count > 0)
    .filter((c) => !needsAttention.some((n) => n.thread_id === c.thread_id))
    .filter((c) => !pinned.some((p) => p.thread_id === c.thread_id))
    .slice(0, 8) ?? []

  // top senders
  const senderMap = new Map<string, { name: string; email: string; count: number }>()
  for (const c of data?.conversations ?? []) {
    for (const p of c.participants) {
      if (p === auth?.address) continue
      const email = extractEmail(p)
      const existing = senderMap.get(email)
      if (existing) {
        existing.count++
      } else {
        senderMap.set(email, { name: extractName(p), email, count: 1 })
      }
    }
  }
  const topSenders = [...senderMap.values()]
    .sort((a, b) => b.count - a.count)
    .slice(0, 6)

  // category stats
  const categoryData = data?.categories
    .filter((c) => c.count > 0)
    .sort((a, b) => b.count - a.count) ?? []
  const totalCategorized = categoryData.reduce((s, c) => s + c.count, 0)

  const displayName = auth?.display_name || auth?.address?.split('@')[0] || ''

  if (loading) {
    return (
      <Panel>
        <div className="flex h-full items-center justify-center">
          <div className="animate-pulse text-sm text-[var(--color-text-tertiary)]">Loading...</div>
        </div>
      </Panel>
    )
  }

  return (
    <PanelRow>
      <Panel>
        <Scroll className="p-4 md:p-6">
          {/* greeting + actions */}
          <div className="mb-6 flex items-start justify-between">
            <div>
              <h1 className="text-xl font-semibold text-[var(--color-text-primary)]">
                {greeting}, {displayName}
              </h1>
              <p className="mt-1 text-sm text-[var(--color-text-tertiary)]">
                {new Date().toLocaleDateString('en', { weekday: 'long', month: 'long', day: 'numeric', year: 'numeric' })}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={() => load(true)}
                className={cn(
                  'flex h-8 w-8 items-center justify-center rounded-md text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]',
                  refreshing && 'animate-spin',
                )}
                title="Refresh"
              >
                <RefreshCw className="h-4 w-4" />
              </button>
              <button
                onClick={handleCompose}
                className="flex items-center gap-1.5 rounded-md bg-[var(--color-brand-primary)] px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-[var(--color-brand-hover)]"
              >
                <Pen className="h-3.5 w-3.5" />
                Compose
              </button>
            </div>
          </div>

          {/* stat cards */}
          <div className="mb-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
            <StatCard
              icon={MailOpen}
              label="Unread"
              value={totalUnread}
              color="brand"
              onClick={() => navigate('/mail')}
            />
            <StatCard
              icon={Clock}
              label="Today"
              value={todayCount}
              color="info"
              onClick={() => navigate('/mail')}
            />
            <StatCard
              icon={Star}
              label="Starred"
              value={starredCount}
              color="warning"
              onClick={() => navigate('/mail')}
            />
            <StatCard
              icon={AlertTriangle}
              label="Important"
              value={importantCount}
              color="danger"
              onClick={() => navigate('/mail')}
            />
          </div>

          {/* main content grid */}
          <div className="grid gap-6 lg:grid-cols-3">
            {/* left column */}
            <div className="space-y-6 lg:col-span-2">
              {/* pinned */}
              {pinned.length > 0 && (
                <Section title="Pinned" icon={Pin}>
                  <div className="space-y-0.5">
                    {pinned.map((c) => (
                      <ConversationRow key={c.thread_id} conversation={c} onClick={() => goToThread(c.thread_id)} />
                    ))}
                  </div>
                </Section>
              )}

              {/* needs attention */}
              {needsAttention.length > 0 && (
                <Section
                  title="Needs Attention"
                  icon={AlertTriangle}
                  action={{ label: 'View all', onClick: () => navigate('/mail') }}
                >
                  <div className="space-y-0.5">
                    {needsAttention.map((c) => (
                      <ConversationRow key={c.thread_id} conversation={c} onClick={() => goToThread(c.thread_id)} />
                    ))}
                  </div>
                </Section>
              )}

              {/* recent unread */}
              {recentUnread.length > 0 && (
                <Section
                  title="Recent"
                  icon={Mail}
                  action={{ label: 'Open inbox', onClick: () => navigate('/mail') }}
                >
                  <div className="space-y-0.5">
                    {recentUnread.map((c) => (
                      <ConversationRow key={c.thread_id} conversation={c} onClick={() => goToThread(c.thread_id)} />
                    ))}
                  </div>
                </Section>
              )}

              {/* all caught up */}
              {needsAttention.length === 0 && recentUnread.length === 0 && pinned.length === 0 && (
                <Section title="Inbox" icon={Mail}>
                  <div className="flex flex-col items-center gap-2 py-8 text-[var(--color-text-tertiary)]">
                    <Shield className="h-8 w-8" aria-hidden="true" />
                    <p className="text-sm">All caught up — no unread emails</p>
                    <button
                      onClick={handleCompose}
                      className="mt-2 flex items-center gap-1.5 rounded-md bg-[var(--color-brand-subtle)] px-3 py-1.5 text-sm font-medium text-[var(--color-brand-primary)] transition-colors hover:bg-[var(--color-brand-primary)] hover:text-white"
                    >
                      <Plus className="h-3.5 w-3.5" />
                      Compose new email
                    </button>
                  </div>
                </Section>
              )}
            </div>

            {/* right column: insights */}
            <div className="space-y-6">
              {/* category breakdown */}
              {categoryData.length > 0 && (
                <Section title="Categories" icon={TrendingUp}>
                  <div className="space-y-2.5">
                    {categoryData.map((cat) => (
                      <CategoryBar
                        key={cat.category}
                        category={cat.category}
                        count={cat.count}
                        total={totalCategorized}
                      />
                    ))}
                  </div>
                </Section>
              )}

              {/* top contacts */}
              {topSenders.length > 0 && (
                <Section title="Top Contacts" icon={Users}>
                  <div className="space-y-0.5">
                    {topSenders.map((s) => (
                      <div key={s.email} className="flex items-center gap-2.5 rounded-md px-2 py-1.5 transition-colors hover:bg-[var(--color-hover)]">
                        <SenderAvatar sender={`${s.name} <${s.email}>`} size={28} />
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-sm font-medium text-[var(--color-text-primary)]">{s.name}</p>
                          <p className="truncate text-xs text-[var(--color-text-tertiary)]">{s.email}</p>
                        </div>
                        <span className="shrink-0 rounded-full bg-[var(--color-bg-sunken)] px-1.5 py-0.5 text-[10px] tabular-nums text-[var(--color-text-tertiary)]">{s.count}</span>
                      </div>
                    ))}
                  </div>
                </Section>
              )}

              {/* folder stats */}
              {data && data.folders.length > 0 && (
                <Section title="Folders" icon={Mail}>
                  <div className="space-y-0.5">
                    {data.folders.filter((f) => f.total > 0).slice(0, 8).map((f) => (
                      <div key={f.name} className="flex items-center justify-between rounded-md px-2 py-1.5 text-sm transition-colors hover:bg-[var(--color-hover)]">
                        <span className="text-[var(--color-text-secondary)]">{f.name}</span>
                        <div className="flex items-center gap-2">
                          {f.unseen > 0 && (
                            <span className="rounded-full bg-[var(--color-brand-subtle)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-brand-primary)]">
                              {f.unseen}
                            </span>
                          )}
                          <span className="tabular-nums text-xs text-[var(--color-text-tertiary)]">{f.total}</span>
                        </div>
                      </div>
                    ))}
                  </div>
                </Section>
              )}
            </div>
          </div>
        </Scroll>
      </Panel>
    </PanelRow>
  )
}

// --- sub-components ---

const COLOR_MAP = {
  brand: 'bg-[var(--color-brand-subtle)] text-[var(--color-brand-primary)]',
  info: 'bg-blue-500/10 text-blue-500',
  warning: 'bg-amber-500/10 text-amber-500',
  danger: 'bg-red-500/10 text-red-500',
} as const

function StatCard({ icon: Icon, label, value, color, onClick }: {
  icon: typeof Mail
  label: string
  value: number
  color: keyof typeof COLOR_MAP
  onClick?: () => void
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'flex items-center gap-3 rounded-lg border border-[var(--color-border-default)] px-4 py-3 text-left transition-colors',
        onClick ? 'cursor-pointer hover:bg-[var(--color-hover)]' : 'cursor-default',
      )}
    >
      <div className={cn('flex h-9 w-9 items-center justify-center rounded-lg', COLOR_MAP[color])}>
        <Icon className="h-4.5 w-4.5" aria-hidden="true" />
      </div>
      <div>
        <p className="text-2xl font-semibold tabular-nums text-[var(--color-text-primary)]">{value}</p>
        <p className="text-xs text-[var(--color-text-tertiary)]">{label}</p>
      </div>
    </button>
  )
}

function Section({ title, icon: Icon, action, children }: {
  title: string
  icon: typeof Mail
  action?: { label: string; onClick: () => void }
  children: React.ReactNode
}) {
  return (
    <div className="rounded-lg border border-[var(--color-border-default)]">
      <div className="flex items-center justify-between border-b border-[var(--color-border-default)] px-4 py-2.5">
        <div className="flex items-center gap-2">
          <Icon className="h-4 w-4 text-[var(--color-text-tertiary)]" aria-hidden="true" />
          <h3 className="text-sm font-medium text-[var(--color-text-primary)]">{title}</h3>
        </div>
        {action && (
          <button
            onClick={action.onClick}
            className="flex items-center gap-1 text-xs text-[var(--color-brand-primary)] transition-colors hover:text-[var(--color-brand-hover)]"
          >
            {action.label}
            <ChevronRight className="h-3 w-3" />
          </button>
        )}
      </div>
      <div className="p-2">
        {children}
      </div>
    </div>
  )
}

function ConversationRow({ conversation: c, onClick }: {
  conversation: ConversationSummary
  onClick: () => void
}) {
  const sender = c.participants[0] ?? ''
  const isUnread = c.unread_count > 0
  return (
    <button
      onClick={onClick}
      className="flex w-full items-center gap-3 rounded-md px-2 py-2 text-left transition-colors hover:bg-[var(--color-hover)]"
    >
      <SenderAvatar sender={sender} size={32} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className={cn(
            'truncate text-sm text-[var(--color-text-primary)]',
            isUnread ? 'font-semibold' : 'font-medium',
          )}>
            {extractName(sender)}
          </span>
          <CategoryBadge category={c.category} />
          {c.flagged && <Star className="h-3 w-3 shrink-0 fill-amber-500 text-amber-500" aria-label="Starred" />}
          {c.pinned && <Pin className="h-3 w-3 shrink-0 text-[var(--color-text-tertiary)]" aria-label="Pinned" />}
        </div>
        <p className={cn(
          'truncate text-xs',
          isUnread ? 'font-medium text-[var(--color-text-primary)]' : 'text-[var(--color-text-secondary)]',
        )}>
          {c.subject || '(no subject)'}
        </p>
        {c.snippet && (
          <p className="mt-0.5 truncate text-xs text-[var(--color-text-tertiary)]">{c.snippet}</p>
        )}
      </div>
      <div className="flex shrink-0 flex-col items-end gap-1">
        <span className="text-xs tabular-nums text-[var(--color-text-tertiary)]">{formatRelative(c.last_date)}</span>
        {isUnread && (
          <span className="flex h-4.5 min-w-4.5 items-center justify-center rounded-full bg-[var(--color-brand-primary)] px-1 text-[10px] font-medium text-white">
            {c.unread_count}
          </span>
        )}
      </div>
    </button>
  )
}

const CATEGORY_COLORS: Record<string, string> = {
  personal: 'bg-blue-500',
  notification: 'bg-purple-500',
  promotion: 'bg-amber-500',
  general: 'bg-gray-400',
  spam: 'bg-red-500',
  scam: 'bg-red-700',
}

function pctToWidth(pct: number): string {
  if (pct >= 95) return 'w-full'
  if (pct >= 85) return 'w-[85%]'
  if (pct >= 75) return 'w-3/4'
  if (pct >= 65) return 'w-[65%]'
  if (pct >= 55) return 'w-[55%]'
  if (pct >= 45) return 'w-[45%]'
  if (pct >= 35) return 'w-[35%]'
  if (pct >= 25) return 'w-1/4'
  if (pct >= 15) return 'w-[15%]'
  if (pct >= 8) return 'w-[10%]'
  if (pct >= 3) return 'w-[5%]'
  if (pct > 0) return 'w-[3%]'
  return 'w-0'
}

function CategoryBar({ category, count, total }: {
  category: string
  count: number
  total: number
}) {
  const pct = total > 0 ? Math.round((count / total) * 100) : 0
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-xs">
        <span className="capitalize text-[var(--color-text-secondary)]">{category}</span>
        <span className="tabular-nums text-[var(--color-text-tertiary)]">{count} ({pct}%)</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-[var(--color-bg-sunken)]">
        <div className={cn('h-full rounded-full transition-all', CATEGORY_COLORS[category] ?? 'bg-gray-400', pctToWidth(pct))} />
      </div>
    </div>
  )
}
