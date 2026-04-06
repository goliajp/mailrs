import type { CategoryCount, ConversationSummary } from '@/lib/types'

import { ScrollArea } from '@goliapkg/gds'
import { useAtomValue, useSetAtom } from 'jotai'
import {
  AlertTriangle,
  ChevronRight,
  Clock,
  Mail,
  MailOpen,
  Pen,
  Pin,
  Plus,
  RefreshCw,
  Search,
  Shield,
  ShieldAlert,
  Star,
  TrendingUp,
  Users,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'

import { CategoryBadge } from '@/components/category-badge'
import { SenderAvatar } from '@/components/sender-avatar'
import { MPane, MPaneGroup } from '@/layouts/pane'
import { fetchJson } from '@/lib/api'
import { cn } from '@/lib/cn'
import { authAtom } from '@/store/auth'
import { composingNewAtom, searchQueryAtom, selectedThreadIdAtom } from '@/store/chat'

type DashboardData = {
  conversations: ConversationSummary[]
  folders: FolderInfo[]
  stats: MailStats | null
}

type FolderInfo = { name: string; total: number; unseen: number }

type MailStats = {
  categories: CategoryCount[]
  storage_bytes: number
  total_messages: number
  unread_messages: number
}

const REFRESH_INTERVAL = 60_000

export function Dashboard() {
  const auth = useAtomValue(authAtom)
  const greeting = useGreeting()
  const navigate = useNavigate()
  const setSelectedThread = useSetAtom(selectedThreadIdAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const setSearchQuery = useSetAtom(searchQueryAtom)
  const [data, setData] = useState<DashboardData | null>(null)
  const [searchInput, setSearchInput] = useState('')
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const intervalRef = useRef<ReturnType<typeof setInterval>>(null)

  const load = useCallback(async (silent = false) => {
    if (!silent) setLoading(true)
    else setRefreshing(true)
    try {
      const [conversations, stats, folders] = await Promise.all([
        fetchJson<ConversationSummary[]>('/conversations?limit=200'),
        fetchJson<MailStats>('/mail/stats').catch(() => null),
        fetchJson<FolderInfo[]>('/mail/folders'),
      ])
      setData({ conversations, folders, stats })
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
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current)
    }
  }, [load])

  const goToThread = useCallback(
    (threadId: string) => {
      setSelectedThread(threadId)
      navigate('/mail')
    },
    [navigate, setSelectedThread]
  )

  const handleCompose = useCallback(() => {
    setComposingNew(true)
    navigate('/mail')
  }, [navigate, setComposingNew])

  const handleSearch = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault()
      if (searchInput.trim()) {
        setSearchQuery(searchInput.trim())
        navigate('/mail')
      }
    },
    [searchInput, setSearchQuery, navigate]
  )

  const totalUnread =
    data?.stats?.unread_messages ?? data?.folders.find((f) => f.name === 'INBOX')?.unseen ?? 0
  const totalMessages = data?.stats?.total_messages ?? 0
  const storageBytes = data?.stats?.storage_bytes ?? 0
  const {
    importantCount,
    needsAttention,
    pinned,
    recentUnread,
    securityAlerts,
    starredCount,
    todayCount,
    topSenders,
  } = useMemo(() => {
    const convos = data?.conversations ?? []
    const todayTs = todayStart()
    const pinnedList = convos.filter((c) => c.pinned).slice(0, 5)
    const pinnedIds = new Set(pinnedList.map((c) => c.thread_id))

    const attentionList = convos
      .filter(
        (c) =>
          c.unread_count > 0 &&
          (c.importance_level === 'high' || c.flagged) &&
          !pinnedIds.has(c.thread_id)
      )
      .slice(0, 8)
    const attentionIds = new Set(attentionList.map((c) => c.thread_id))

    const senderMap = new Map<string, { count: number; email: string; name: string }>()
    for (const c of convos) {
      for (const p of c.participants) {
        if (p === auth?.address) continue
        const email = extractEmail(p)
        const existing = senderMap.get(email)
        if (existing) existing.count++
        else senderMap.set(email, { count: 1, email, name: extractName(p) })
      }
    }

    return {
      importantCount: convos.filter((c) => c.importance_level === 'high' && c.unread_count > 0)
        .length,
      needsAttention: attentionList,
      pinned: pinnedList,
      recentUnread: convos
        .filter(
          (c) => c.unread_count > 0 && !pinnedIds.has(c.thread_id) && !attentionIds.has(c.thread_id)
        )
        .slice(0, 8),
      securityAlerts: convos
        .filter((c) => c.unread_count > 0 && (c.category === 'spam' || c.category === 'scam'))
        .slice(0, 5),
      starredCount: convos.filter((c) => c.flagged).length,
      todayCount: convos.filter((c) => c.last_date >= todayTs).length,
      topSenders: [...senderMap.values()].sort((a, b) => b.count - a.count).slice(0, 6),
    }
  }, [data?.conversations, auth?.address])

  // category stats (prefer stats endpoint, fallback to separate categories call)
  const categoryData = (data?.stats?.categories ?? [])
    .filter((c) => c.count > 0)
    .sort((a, b) => b.count - a.count)
  const totalCategorized = categoryData.reduce((s, c) => s + c.count, 0)

  const displayName = auth?.display_name || auth?.address?.split('@')[0] || ''

  if (loading) {
    return (
      <MPaneGroup>
        <MPane>
          <ScrollArea className="p-4 md:p-6">
            <div className="animate-pulse space-y-6">
              <div className="space-y-2">
                <div className="bg-border h-6 w-48 rounded" />
                <div className="bg-border h-4 w-64 rounded" />
              </div>
              <div className="bg-border h-10 w-full rounded-lg" />
              <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
                {Array.from({ length: 4 }).map((_, i) => (
                  <div className="bg-border h-16 rounded-lg" key={i} />
                ))}
              </div>
              <div className="grid gap-6 lg:grid-cols-3">
                <div className="space-y-3 lg:col-span-2">
                  {Array.from({ length: 4 }).map((_, i) => (
                    <div className="bg-border h-14 rounded-lg" key={i} />
                  ))}
                </div>
                <div className="space-y-3">
                  {Array.from({ length: 3 }).map((_, i) => (
                    <div className="bg-border h-24 rounded-lg" key={i} />
                  ))}
                </div>
              </div>
            </div>
          </ScrollArea>
        </MPane>
      </MPaneGroup>
    )
  }

  return (
    <MPaneGroup>
      <MPane>
        <ScrollArea className="p-4 md:p-6">
          {/* greeting + actions */}
          <div className="mb-6 flex items-start justify-between">
            <div>
              <h1 className="text-fg text-xl font-semibold">
                {greeting}, {displayName}
              </h1>
              <p className="text-fg-muted mt-1 text-sm">
                {new Date().toLocaleDateString('en', {
                  day: 'numeric',
                  month: 'long',
                  weekday: 'long',
                  year: 'numeric',
                })}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <button
                className={cn(
                  'text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex h-8 w-8 items-center justify-center rounded-md transition-colors',
                  refreshing && 'animate-spin'
                )}
                onClick={() => load(true)}
                title="Refresh"
              >
                <RefreshCw className="h-4 w-4" />
              </button>
              <button
                className="bg-accent hover:bg-accent-hover flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors"
                onClick={handleCompose}
              >
                <Pen className="h-3.5 w-3.5" />
                Compose
              </button>
            </div>
          </div>

          {/* search bar */}
          <form className="mb-6" onSubmit={handleSearch}>
            <div className="relative">
              <Search
                aria-hidden="true"
                className="text-fg-muted pointer-events-none absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2"
              />
              <input
                className="border-border bg-surface text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent w-full rounded-lg border py-2.5 pr-4 pl-9 text-sm focus:ring-1 focus:outline-none"
                onChange={(e) => setSearchInput(e.target.value)}
                placeholder="Search emails..."
                type="text"
                value={searchInput}
              />
            </div>
          </form>

          {/* stat cards */}
          <div className="mb-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
            <StatCard
              color="brand"
              icon={MailOpen}
              label="Unread"
              onClick={() => navigate('/mail')}
              value={totalUnread}
            />
            <StatCard
              color="info"
              icon={Clock}
              label="Today"
              onClick={() => navigate('/mail')}
              value={todayCount}
            />
            <StatCard
              color="warning"
              icon={Star}
              label="Starred"
              onClick={() => navigate('/mail')}
              value={starredCount}
            />
            <StatCard
              color="danger"
              icon={AlertTriangle}
              label="Important"
              onClick={() => navigate('/mail')}
              value={importantCount}
            />
          </div>

          {/* main content grid */}
          <div className="grid gap-6 lg:grid-cols-3">
            {/* left column */}
            <div className="space-y-6 lg:col-span-2">
              {/* pinned */}
              {pinned.length > 0 && (
                <Section icon={Pin} title="Pinned">
                  <div className="space-y-0.5">
                    {pinned.map((c) => (
                      <ConversationRow
                        conversation={c}
                        key={c.thread_id}
                        onClick={() => goToThread(c.thread_id)}
                      />
                    ))}
                  </div>
                </Section>
              )}

              {/* needs attention */}
              {needsAttention.length > 0 && (
                <Section
                  action={{
                    label: 'View all',
                    onClick: () => navigate('/mail'),
                  }}
                  icon={AlertTriangle}
                  title="Needs Attention"
                >
                  <div className="space-y-0.5">
                    {needsAttention.map((c) => (
                      <ConversationRow
                        conversation={c}
                        key={c.thread_id}
                        onClick={() => goToThread(c.thread_id)}
                      />
                    ))}
                  </div>
                </Section>
              )}

              {/* recent unread */}
              {recentUnread.length > 0 && (
                <Section
                  action={{
                    label: 'Open inbox',
                    onClick: () => navigate('/mail'),
                  }}
                  icon={Mail}
                  title="Recent"
                >
                  <div className="space-y-0.5">
                    {recentUnread.map((c) => (
                      <ConversationRow
                        conversation={c}
                        key={c.thread_id}
                        onClick={() => goToThread(c.thread_id)}
                      />
                    ))}
                  </div>
                </Section>
              )}

              {/* all caught up */}
              {needsAttention.length === 0 && recentUnread.length === 0 && pinned.length === 0 && (
                <>
                  <Section icon={Mail} title="Inbox">
                    <div className="text-fg-muted flex flex-col items-center gap-2 py-6">
                      <Shield aria-hidden="true" className="h-8 w-8" />
                      <p className="text-sm">All caught up — no unread emails</p>
                      <button
                        className="bg-accent/10 text-accent hover:bg-accent mt-2 flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors hover:text-white"
                        onClick={handleCompose}
                      >
                        <Plus className="h-3.5 w-3.5" />
                        Compose new email
                      </button>
                    </div>
                  </Section>
                  {/* show recent conversations even if all read */}
                  {(data?.conversations.slice(0, 5).length ?? 0) > 0 && (
                    <Section
                      action={{
                        label: 'Open inbox',
                        onClick: () => navigate('/mail'),
                      }}
                      icon={Clock}
                      title="Recent Activity"
                    >
                      <div className="space-y-0.5">
                        {data!.conversations.slice(0, 5).map((c) => (
                          <ConversationRow
                            conversation={c}
                            key={c.thread_id}
                            onClick={() => goToThread(c.thread_id)}
                          />
                        ))}
                      </div>
                    </Section>
                  )}
                </>
              )}
            </div>

            {/* right column: insights */}
            <div className="space-y-6">
              {/* security alerts */}
              {securityAlerts.length > 0 && (
                <Section icon={ShieldAlert} title="Security Alerts">
                  <div className="space-y-0.5">
                    {securityAlerts.map((c) => (
                      <button
                        className="hover:bg-bg-secondary flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left transition-colors"
                        key={c.thread_id}
                        onClick={() => goToThread(c.thread_id)}
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

              {/* category breakdown */}
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

              {/* top contacts */}
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

              {/* mailbox overview */}
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
                      <span className="text-fg font-medium tabular-nums">
                        {formatBytes(storageBytes)}
                      </span>
                    </div>
                  </div>
                </Section>
              )}

              {/* folder stats */}
              {data && data.folders.length > 0 && (
                <Section icon={Mail} title="Folders">
                  <div className="space-y-0.5">
                    {data.folders
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
          </div>

          {/* keyboard shortcuts hint */}
          <div className="text-fg-muted mt-8 flex flex-wrap items-center justify-center gap-x-6 gap-y-2 text-xs">
            <Shortcut keys="⌘K" label="Command palette" />
            <Shortcut keys="C" label="Compose" />
            <Shortcut keys="/" label="Search" />
            <Shortcut keys="J/K" label="Navigate" />
            <Shortcut keys="?" label="All shortcuts" />
          </div>
        </ScrollArea>
      </MPane>
    </MPaneGroup>
  )
}

function extractEmail(sender: string): string {
  const m = sender.match(/<([^>]+)>/)
  return m ? m[1] : sender
}

function extractName(sender: string): string {
  const m = sender.match(/^"?([^"<]+)"?\s*</)
  return m ? m[1].trim() : sender.split('@')[0]
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  const value = bytes / Math.pow(1024, i)
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`
}

function formatRelative(ts: number): string {
  const now = Math.floor(Date.now() / 1000)
  const diff = now - ts
  if (diff < 60) return 'just now'
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
  if (diff < 604800) return `${Math.floor(diff / 86400)}d ago`
  return new Date(ts * 1000).toLocaleDateString('en', {
    day: 'numeric',
    month: 'short',
  })
}

function Shortcut({ keys, label }: { keys: string; label: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <kbd className="border-border bg-surface text-fg-secondary rounded border px-1.5 py-0.5 font-mono text-[10px]">
        {keys}
      </kbd>
      <span>{label}</span>
    </span>
  )
}

function todayStart(): number {
  const d = new Date()
  d.setHours(0, 0, 0, 0)
  return Math.floor(d.getTime() / 1000)
}

// --- sub-components ---

function useGreeting() {
  const hour = new Date().getHours()
  if (hour < 6) return 'Good night'
  if (hour < 12) return 'Good morning'
  if (hour < 18) return 'Good afternoon'
  return 'Good evening'
}

const COLOR_MAP = {
  brand: 'bg-accent/10 text-accent',
  danger: 'bg-red-500/10 text-red-500',
  info: 'bg-blue-500/10 text-blue-500',
  warning: 'bg-amber-500/10 text-amber-500',
} as const

function ConversationRow({
  conversation: c,
  onClick,
}: {
  conversation: ConversationSummary
  onClick: () => void
}) {
  const sender = c.participants[0] ?? ''
  const isUnread = c.unread_count > 0
  return (
    <button
      className="hover:bg-bg-secondary flex w-full items-center gap-3 rounded-md px-2 py-2 text-left transition-colors"
      onClick={onClick}
    >
      <SenderAvatar sender={sender} size={32} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span
            className={cn('text-fg truncate text-sm', isUnread ? 'font-semibold' : 'font-medium')}
          >
            {extractName(sender)}
          </span>
          <CategoryBadge category={c.category} />
          {c.flagged && (
            <Star aria-label="Starred" className="h-3 w-3 shrink-0 fill-amber-500 text-amber-500" />
          )}
          {c.pinned && <Pin aria-label="Pinned" className="text-fg-muted h-3 w-3 shrink-0" />}
        </div>
        <p
          className={cn('truncate text-xs', isUnread ? 'text-fg font-medium' : 'text-fg-secondary')}
        >
          {c.subject || '(no subject)'}
        </p>
        {c.snippet && <p className="text-fg-muted mt-0.5 truncate text-xs">{c.snippet}</p>}
      </div>
      <div className="flex shrink-0 flex-col items-end gap-1">
        <span className="text-fg-muted text-xs tabular-nums">{formatRelative(c.last_date)}</span>
        {isUnread && (
          <span className="bg-accent flex h-4.5 min-w-4.5 items-center justify-center rounded-full px-1 text-[10px] font-medium text-white">
            {c.unread_count}
          </span>
        )}
      </div>
    </button>
  )
}

function Section({
  action,
  children,
  icon: Icon,
  title,
}: {
  action?: { label: string; onClick: () => void }
  children: React.ReactNode
  icon: typeof Mail
  title: string
}) {
  return (
    <div className="border-border rounded-lg border">
      <div className="border-border flex items-center justify-between border-b px-4 py-2.5">
        <div className="flex items-center gap-2">
          <Icon aria-hidden="true" className="text-fg-muted h-4 w-4" />
          <h3 className="text-fg text-sm font-medium">{title}</h3>
        </div>
        {action && (
          <button
            className="text-accent hover:text-accent-hover flex items-center gap-1 text-xs transition-colors"
            onClick={action.onClick}
          >
            {action.label}
            <ChevronRight className="h-3 w-3" />
          </button>
        )}
      </div>
      <div className="p-2">{children}</div>
    </div>
  )
}

function StatCard({
  color,
  icon: Icon,
  label,
  onClick,
  value,
}: {
  color: keyof typeof COLOR_MAP
  icon: typeof Mail
  label: string
  onClick?: () => void
  value: number
}) {
  return (
    <button
      className={cn(
        'border-border flex items-center gap-3 rounded-lg border px-4 py-3 text-left transition-colors',
        onClick ? 'hover:bg-bg-secondary cursor-pointer' : 'cursor-default'
      )}
      onClick={onClick}
    >
      <div className={cn('flex h-9 w-9 items-center justify-center rounded-lg', COLOR_MAP[color])}>
        <Icon aria-hidden="true" className="h-4.5 w-4.5" />
      </div>
      <div>
        <p className="text-fg text-2xl font-semibold tabular-nums">{value}</p>
        <p className="text-fg-muted text-xs">{label}</p>
      </div>
    </button>
  )
}

const CATEGORY_COLORS: Record<string, string> = {
  general: 'bg-gray-400',
  notification: 'bg-purple-500',
  personal: 'bg-blue-500',
  promotion: 'bg-amber-500',
  scam: 'bg-red-700',
  spam: 'bg-red-500',
}

function CategoryBar({
  category,
  count,
  total,
}: {
  category: string
  count: number
  total: number
}) {
  const pct = total > 0 ? Math.round((count / total) * 100) : 0
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-xs">
        <span className="text-fg-secondary capitalize">{category}</span>
        <span className="text-fg-muted tabular-nums">
          {count} ({pct}%)
        </span>
      </div>
      <div className="bg-bg-secondary h-1.5 overflow-hidden rounded-full">
        <div
          className={cn(
            'h-full rounded-full transition-all',
            CATEGORY_COLORS[category] ?? 'bg-gray-400',
            pctToWidth(pct)
          )}
        />
      </div>
    </div>
  )
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
