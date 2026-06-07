import type { CategoryCount, ConversationSummary } from '@/lib/types'

import { ScrollArea } from '@goliapkg/gds'
import { useQueries } from '@tanstack/react-query'
import { useAtomValue, useSetAtom } from 'jotai'
import { AlertTriangle, Clock, MailOpen, Pen, RefreshCw, Search, Star } from 'lucide-react'
import { useCallback, useMemo, useState } from 'react'
import { useNavigate } from 'react-router'

import {
  extractEmail,
  extractName,
  InsightsColumn,
  MainColumn,
  Shortcut,
  StatCard,
  todayStart,
  useGreeting,
} from '@/components/dashboard'
import { fetchJson } from '@/lib/api'
import { cn } from '@/lib/cn'
import { dashboardKeys } from '@/lib/query-keys'
import { authAtom } from '@/store/auth'
import {
  composeReplySourceAtom,
  composingNewAtom,
  quickFilterAtom,
  searchQueryAtom,
  selectedThreadIdAtom,
} from '@/store/chat'

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
  const setComposeReplySource = useSetAtom(composeReplySourceAtom)
  const setSearchQuery = useSetAtom(searchQueryAtom)
  const setQuickFilter = useSetAtom(quickFilterAtom)
  const [searchInput, setSearchInput] = useState('')

  // no separate `loading` state: the only skeleton the user can see is the
  // Suspense fallback that renders during the lazy chunk download (app.tsx
  // → DashboardShellSkeleton). once this component has mounted the chunk
  // is loaded, so we render the real layout immediately. data-driven
  // sections naturally start empty and fill in when the fetch resolves.
  const queries = useQueries({
    queries: [
      {
        queryKey: dashboardKeys.conversations(),
        refetchInterval: REFRESH_INTERVAL,
        queryFn: ({ signal }: { signal: AbortSignal }) =>
          fetchJson<ConversationSummary[]>('/conversations?limit=200', signal),
      },
      {
        queryKey: dashboardKeys.stats(),
        refetchInterval: REFRESH_INTERVAL,
        queryFn: ({ signal }: { signal: AbortSignal }) =>
          fetchJson<MailStats>('/mail/stats', signal).catch(() => null),
      },
      {
        queryKey: dashboardKeys.folders(),
        refetchInterval: REFRESH_INTERVAL,
        queryFn: ({ signal }: { signal: AbortSignal }) =>
          fetchJson<FolderInfo[]>('/mail/folders', signal),
      },
    ],
  })
  const [convosQuery, statsQuery, foldersQuery] = queries
  const data: DashboardData | null = useMemo(() => {
    if (convosQuery.data === undefined && foldersQuery.data === undefined) return null
    return {
      conversations: convosQuery.data ?? [],
      folders: foldersQuery.data ?? [],
      stats: statsQuery.data ?? null,
    }
  }, [convosQuery.data, foldersQuery.data, statsQuery.data])
  const refreshing = queries.some((q) => q.isFetching && !q.isLoading)

  const refetchAll = useCallback(() => {
    void convosQuery.refetch()
    void statsQuery.refetch()
    void foldersQuery.refetch()
  }, [convosQuery, statsQuery, foldersQuery])

  const goToThread = useCallback(
    (threadId: string) => {
      setSelectedThread(threadId)
      navigate('/mail')
    },
    [navigate, setSelectedThread]
  )

  const handleCompose = useCallback(() => {
    setComposeReplySource(null)
    setComposingNew(true)
    navigate('/mail')
  }, [navigate, setComposingNew, setComposeReplySource])

  const handleOpenInbox = useCallback(() => {
    navigate('/mail')
  }, [navigate])

  const handleOpenUnreadInbox = useCallback(() => {
    setQuickFilter('unread')
    navigate('/mail')
  }, [navigate, setQuickFilter])

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

  const categoryData = (data?.stats?.categories ?? [])
    .filter((c) => c.count > 0)
    .sort((a, b) => b.count - a.count)
  const totalCategorized = categoryData.reduce((s, c) => s + c.count, 0)

  const displayName = auth?.display_name || auth?.address?.split('@')[0] || ''

  return (
    // no MPaneGroup/MPane wrapper here: app.tsx's PagePane already renders
    // <MPane> on desktop / a plain scroll div on mobile around this
    // component.
    <ScrollArea className="max-w-full overflow-x-hidden p-4 md:p-6">
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
            aria-busy={refreshing}
            aria-label="Refresh dashboard"
            className={cn(
              'text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex h-8 w-8 items-center justify-center rounded-md transition-colors',
              refreshing && 'animate-spin'
            )}
            onClick={refetchAll}
            title="Refresh"
            type="button"
          >
            <RefreshCw className="h-4 w-4" />
          </button>
          <button
            aria-keyshortcuts="C"
            className="bg-accent hover:bg-accent-hover flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium text-white transition-colors"
            onClick={handleCompose}
            type="button"
          >
            <Pen className="h-3.5 w-3.5" />
            Compose
          </button>
        </div>
      </div>

      {/* search bar */}
      <form className="mb-6" onSubmit={handleSearch} role="search">
        <div className="relative">
          <Search
            aria-hidden="true"
            className="text-fg-muted pointer-events-none absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2"
          />
          <input
            aria-label="Search emails"
            className="border-border bg-surface text-fg placeholder:text-fg-muted focus:border-accent focus:ring-accent w-full rounded-lg border py-2.5 pr-4 pl-9 text-sm focus:ring-1 focus:outline-none"
            onChange={(e) => setSearchInput(e.target.value)}
            placeholder="Search emails..."
            type="search"
            value={searchInput}
          />
        </div>
      </form>

      {/* stat cards — horizontal scroll on mobile, grid on desktop */}
      <div className="scrollbar-hide mb-6 flex gap-3 overflow-x-auto pb-1 md:grid md:grid-cols-2 md:overflow-visible lg:grid-cols-4">
        <StatCard
          color="brand"
          icon={MailOpen}
          label="Unread"
          onClick={handleOpenInbox}
          value={totalUnread}
        />
        <StatCard
          color="info"
          icon={Clock}
          label="Today"
          onClick={handleOpenInbox}
          value={todayCount}
        />
        <StatCard
          color="warning"
          icon={Star}
          label="Starred"
          onClick={handleOpenInbox}
          value={starredCount}
        />
        <StatCard
          color="danger"
          icon={AlertTriangle}
          label="Important"
          onClick={handleOpenInbox}
          value={importantCount}
        />
      </div>

      {/* main content grid */}
      <div className="grid gap-6 lg:grid-cols-3">
        <MainColumn
          conversations={data?.conversations ?? []}
          needsAttention={needsAttention}
          onCompose={handleCompose}
          onOpenInbox={totalUnread > 0 ? handleOpenUnreadInbox : handleOpenInbox}
          onOpenThread={goToThread}
          pinned={pinned}
          recentUnread={recentUnread}
          totalUnread={totalUnread}
        />
        <InsightsColumn
          categoryData={categoryData}
          folders={data?.folders ?? []}
          onOpenThread={goToThread}
          securityAlerts={securityAlerts}
          storageBytes={storageBytes}
          topSenders={topSenders}
          totalCategorized={totalCategorized}
          totalMessages={totalMessages}
        />
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
  )
}
