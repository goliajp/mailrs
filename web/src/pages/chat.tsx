import type { MailListFilters } from '@/lib/query-keys'
import type { ConversationSummary } from '@/lib/types'

import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'

import { ConversationList } from '@/components/conversation-list'
import { KeyboardShortcutsDialog } from '@/components/keyboard-shortcuts-dialog'
import { MobileMail } from '@/components/mobile-mail'
import { NewConversation } from '@/components/new-conversation'
import { ThreadView } from '@/components/thread-view'
import { useKeyboardNav } from '@/hooks/use-keyboard-nav'
import { shallowEqualConvo, useMailEvents } from '@/hooks/use-mail-events'
import { useConversationsQuery } from '@/hooks/use-mail-queries'
import { MPane, MPaneGroup } from '@/layouts/pane'
import { authAtom } from '@/store/auth'
import {
  categoryFilterAtom,
  composingNewAtom,
  conversationsAtom,
  folderAtom,
  hasMoreAtom,
  importanceSectionAtom,
  initialLoadingAtom,
  loadingMoreAtom,
  mobileViewAtom,
  quickFilterAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  selectedThreadIdAtom,
  shortcutsDialogOpenAtom,
  showArchivedAtom,
} from '@/store/chat'

export function Chat() {
  const auth = useAtomValue(authAtom)
  const composingNew = useAtomValue(composingNewAtom)
  const [conversations, setConversations] = useAtom(conversationsAtom)
  const searchQuery = useAtomValue(searchQueryAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const folder = useAtomValue(folderAtom)
  const setHasMore = useSetAtom(hasMoreAtom)
  const setLoadingMore = useSetAtom(loadingMoreAtom)
  const setInitialLoading = useSetAtom(initialLoadingAtom)
  const [mobileView, setMobileView] = useAtom(mobileViewAtom)
  const [shortcutsOpen, setShortcutsOpen] = useAtom(shortcutsDialogOpenAtom)
  const showArchived = useAtomValue(showArchivedAtom)
  const [quickFilter, setQuickFilter] = useAtom(quickFilterAtom)
  const [importanceSection, setImportanceSection] = useAtom(importanceSectionAtom)
  const [selectedThreadId, setSelectedThreadId] = useAtom(selectedThreadIdAtom)
  const setFolder = useSetAtom(folderAtom)
  const setCategoryFilter = useSetAtom(categoryFilterAtom)
  const [searchParams, setSearchParams] = useSearchParams()

  // Single effect that owns the URL <-> atom sync:
  //   - first run: restore atom values from URL params (and skip writing
  //     back, so we don't clobber the URL before our setX calls flush)
  //   - subsequent runs: write atom values into URL
  // Keeping it in one effect avoids the race between a separate "restore"
  // and "sync" pair where the sync's first invocation captures default
  // atom values and overwrites the URL to empty before the restore's
  // setX updates have re-rendered.
  const initializedRef = useRef(false)
  useEffect(() => {
    if (!initializedRef.current) {
      initializedRef.current = true
      const urlThread = searchParams.get('thread')
      const urlView = searchParams.get('view') as
        | 'conversation'
        | 'list'
        | 'reply'
        | 'thread'
        | null
      const urlFolder = searchParams.get('folder')
      const urlTab = searchParams.get('tab')
      const urlCat = searchParams.get('cat')
      if (urlThread) setSelectedThreadId(urlThread)
      if (urlView) setMobileView(urlView)
      if (urlFolder === 'Drafts' || urlFolder === 'Sent' || urlFolder === 'Trash') {
        setFolder(urlFolder)
      }
      if (urlTab === 'unread' || urlTab === 'starred' || urlTab === 'attachment') {
        setQuickFilter(urlTab)
      } else if (urlTab === 'action' || urlTab === 'important' || urlTab === 'other') {
        setImportanceSection(urlTab)
      }
      if (urlCat) setCategoryFilter(urlCat)
      return
    }
    const params = new URLSearchParams()
    if (selectedThreadId) params.set('thread', selectedThreadId)
    if (mobileView !== 'list') params.set('view', mobileView)
    if (folder) params.set('folder', folder)
    if (quickFilter !== 'all') params.set('tab', quickFilter)
    else if (importanceSection) params.set('tab', importanceSection)
    if (categoryFilter) params.set('cat', categoryFilter)
    const newSearch = params.toString()
    const currentSearch = searchParams.toString()
    if (newSearch !== currentSearch) {
      setSearchParams(params, { replace: true })
    }
  }, [
    selectedThreadId,
    mobileView,
    folder,
    quickFilter,
    importanceSection,
    categoryFilter,
    searchParams,
    setSearchParams,
    setSelectedThreadId,
    setMobileView,
    setFolder,
    setQuickFilter,
    setImportanceSection,
    setCategoryFilter,
  ])

  // request notification permission
  useEffect(() => {
    if (typeof Notification !== 'undefined' && Notification.permission === 'default') {
      Notification.requestPermission()
    }
  }, [])

  // websocket events
  useMailEvents(auth?.address ?? '')

  // keyboard navigation
  useKeyboardNav()

  // Search-only debounce: avoid hammering the server while the user types.
  // For all other filter changes the query swap is instant via RQ cache.
  const [debouncedSearch, setDebouncedSearch] = useState(searchQuery)
  useEffect(() => {
    if (!searchQuery) {
      setDebouncedSearch('')
      return
    }
    const t = setTimeout(() => setDebouncedSearch(searchQuery), 300)
    return () => clearTimeout(t)
  }, [searchQuery])

  const filters: MailListFilters = useMemo(
    () => ({
      archived: showArchived,
      category: categoryFilter,
      domains: selectedDomains.length > 0 ? selectedDomains : undefined,
      folder,
      query: debouncedSearch || undefined,
      section: importanceSection,
      starred: quickFilter === 'starred',
      unread: quickFilter === 'unread',
    }),
    [
      showArchived,
      categoryFilter,
      selectedDomains,
      folder,
      debouncedSearch,
      importanceSection,
      quickFilter,
    ]
  )

  const conversationsQuery = useConversationsQuery(filters)

  // Project the query state onto the legacy atoms so the rest of the UI
  // (conversation-list, thread-view, etc.) keeps working unchanged.
  // Identity-preserving merge keeps memo'd rows from re-rendering when a
  // refetch returns the same payload.
  const flatConversations = useMemo(() => {
    const pages = conversationsQuery.data?.pages ?? []
    const flat: ConversationSummary[] = []
    const seen = new Set<string>()
    for (const page of pages) {
      for (const c of page) {
        if (seen.has(c.thread_id)) continue
        seen.add(c.thread_id)
        flat.push(c)
      }
    }
    return flat
  }, [conversationsQuery.data])

  useEffect(() => {
    setConversations((prev) => {
      const byId = new Map<string, ConversationSummary>()
      for (const c of prev) byId.set(c.thread_id, c)
      let allSame = prev.length === flatConversations.length
      const next = flatConversations.map((f, i) => {
        const existing = byId.get(f.thread_id)
        const kept = existing && shallowEqualConvo(existing, f) ? existing : f
        if (allSame && prev[i] !== kept) allSame = false
        return kept
      })
      return allSame ? prev : next
    })
  }, [flatConversations, setConversations])

  useEffect(() => {
    setInitialLoading(conversationsQuery.isPending && flatConversations.length === 0)
  }, [conversationsQuery.isPending, flatConversations.length, setInitialLoading])

  useEffect(() => {
    setHasMore(conversationsQuery.hasNextPage)
  }, [conversationsQuery.hasNextPage, setHasMore])

  useEffect(() => {
    setLoadingMore(conversationsQuery.isFetchingNextPage)
  }, [conversationsQuery.isFetchingNextPage, setLoadingMore])

  const loadMore = useCallback(() => {
    if (!conversationsQuery.hasNextPage) return Promise.resolve()
    if (conversationsQuery.isFetchingNextPage) return Promise.resolve()
    return conversationsQuery.fetchNextPage().then(() => undefined)
  }, [conversationsQuery])

  const refreshConversations = useCallback(
    () => conversationsQuery.refetch().then(() => undefined),
    [conversationsQuery]
  )

  // auto-select first conversation on desktop (desktop shows list + detail side by side)
  useEffect(() => {
    if (
      window.innerWidth >= 768 &&
      !selectedThreadId &&
      !composingNew &&
      conversations.length > 0
    ) {
      setSelectedThreadId(conversations[0].thread_id)
    }
  }, [conversations, selectedThreadId, composingNew, setSelectedThreadId])

  return (
    <>
      {/* ─── MOBILE: full-screen view switching ─── */}
      <div className="h-full md:hidden">
        {mobileView === 'list' ? (
          <ConversationList
            onLoadMore={loadMore}
            onRefresh={refreshConversations}
            onSelectConversation={() => setMobileView('thread')}
          />
        ) : (
          <MobileMail />
        )}
      </div>

      {/* ─── DESKTOP: side-by-side pane layout (unchanged) ─── */}
      <MPaneGroup className="hidden md:flex">
        <MPane width={480}>
          <ConversationList
            onLoadMore={loadMore}
            onRefresh={refreshConversations}
            onSelectConversation={() => setMobileView('thread')}
          />
        </MPane>

        <MPaneGroup>
          {composingNew ? (
            <MPane>
              <NewConversation />
            </MPane>
          ) : (
            <ThreadView onBack={() => setMobileView('list')} />
          )}
        </MPaneGroup>

        <KeyboardShortcutsDialog onClose={() => setShortcutsOpen(false)} open={shortcutsOpen} />
      </MPaneGroup>
    </>
  )
}
