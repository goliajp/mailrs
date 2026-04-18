import type { ConversationSummary } from '@/lib/types'

import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef } from 'react'
import { useSearchParams } from 'react-router'

import { ConversationList } from '@/components/conversation-list'
import { KeyboardShortcutsDialog } from '@/components/keyboard-shortcuts-dialog'
import { MobileMail } from '@/components/mobile-mail'
import { NewConversation } from '@/components/new-conversation'
import { ThreadView } from '@/components/thread-view'
import { useKeyboardNav } from '@/hooks/use-keyboard-nav'
import { useMailEvents } from '@/hooks/use-mail-events'
import { MPane, MPaneGroup } from '@/layouts/pane'
import { fetchJson } from '@/lib/api'
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

const PAGE_SIZE = 50

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
  const quickFilter = useAtomValue(quickFilterAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const [selectedThreadId, setSelectedThreadId] = useAtom(selectedThreadIdAtom)
  const [searchParams, setSearchParams] = useSearchParams()
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null)
  const firstLoadDone = useRef(false)
  const prevSearchRef = useRef(searchQuery)

  // restore state from URL on mount
  const initializedRef = useRef(false)
  useEffect(() => {
    if (initializedRef.current) return
    initializedRef.current = true
    const urlThread = searchParams.get('thread')
    const urlView = searchParams.get('view') as 'conversation' | 'list' | 'reply' | 'thread' | null
    if (urlThread) setSelectedThreadId(urlThread)
    if (urlView) setMobileView(urlView)
  }, [searchParams, setSelectedThreadId, setMobileView])

  // sync state TO URL when it changes
  useEffect(() => {
    const params = new URLSearchParams()
    if (selectedThreadId) params.set('thread', selectedThreadId)
    if (mobileView !== 'list') params.set('view', mobileView)
    const newSearch = params.toString()
    const currentSearch = searchParams.toString()
    if (newSearch !== currentSearch) {
      setSearchParams(params, { replace: true })
    }
  }, [selectedThreadId, mobileView, searchParams, setSearchParams])

  // keep a ref so loadMore always sees latest
  const conversationsRef = useRef(conversations)
  conversationsRef.current = conversations
  const searchRef = useRef(searchQuery)
  searchRef.current = searchQuery
  const categoryRef = useRef(categoryFilter)
  categoryRef.current = categoryFilter
  const domainsRef = useRef(selectedDomains)
  domainsRef.current = selectedDomains
  const archivedRef = useRef(showArchived)
  archivedRef.current = showArchived
  const folderRef = useRef(folder)
  folderRef.current = folder
  const quickFilterRef = useRef(quickFilter)
  quickFilterRef.current = quickFilter
  const sectionRef = useRef(importanceSection)
  sectionRef.current = importanceSection

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

  // build API path helper
  const buildPath = useCallback(
    (opts?: {
      archived?: boolean
      before?: number
      category?: null | string
      domains?: string[]
      folder?: null | string
      query?: string
      section?: null | string
      starred?: boolean
      unread?: boolean
    }) => {
      const {
        archived,
        before,
        category,
        domains,
        folder: f,
        query,
        section,
        starred,
        unread,
      } = opts ?? {}
      if (query) {
        let path = `/conversations/search?q=${encodeURIComponent(query)}&limit=${PAGE_SIZE}`
        if (category) path += `&category=${encodeURIComponent(category)}`
        if (domains && domains.length > 0)
          path += `&domains=${encodeURIComponent(domains.join(','))}`
        return path
      }
      let path = `/conversations?limit=${PAGE_SIZE}`
      if (before) path += `&before=${before}`
      if (category) path += `&category=${encodeURIComponent(category)}`
      if (domains && domains.length > 0) path += `&domains=${encodeURIComponent(domains.join(','))}`
      if (archived) path += '&archived=true'
      if (f) path += `&folder=${encodeURIComponent(f)}`
      if (unread) path += '&unread=true'
      if (starred) path += '&starred=true'
      if (section) path += `&section=${encodeURIComponent(section)}`
      return path
    },
    []
  )

  // load conversations with optional append mode
  const loadConversations = useCallback(
    async (opts?: {
      append?: boolean
      archived?: boolean
      before?: number
      category?: null | string
      domains?: string[]
      folder?: null | string
      query?: string
      section?: null | string
      starred?: boolean
      unread?: boolean
    }) => {
      const { append } = opts ?? {}
      try {
        const path = buildPath(opts)
        const data = await fetchJson<ConversationSummary[]>(path)

        if (append) {
          setConversations((prev) => {
            const ids = new Set(prev.map((c) => c.thread_id))
            return [...prev, ...data.filter((c) => !ids.has(c.thread_id))]
          })
        } else {
          setConversations(data)
        }

        setHasMore(data.length >= PAGE_SIZE)
      } catch {
        // keep current
      } finally {
        // clear loading on every fetch, not just the first — otherwise a
        // filter-change refetch (which clears conversations before fetching)
        // would leave the UI stuck showing the empty state
        firstLoadDone.current = true
        if (!append) setInitialLoading(false)
      }
    },
    [setConversations, setHasMore, setInitialLoading, buildPath]
  )

  // load more (infinite scroll callback) with reentry guard
  const loadingRef = useRef(false)
  const loadMore = useCallback(async () => {
    if (loadingRef.current) return
    const current = conversationsRef.current
    const last = current[current.length - 1]
    if (!last) return

    loadingRef.current = true
    setLoadingMore(true)
    try {
      await loadConversations({
        append: true,
        archived: archivedRef.current || undefined,
        before: last.last_date,
        category: categoryRef.current,
        domains: domainsRef.current.length > 0 ? domainsRef.current : undefined,
        folder: folderRef.current,
        query: searchRef.current || undefined,
        section: sectionRef.current,
        starred: quickFilterRef.current === 'starred' || undefined,
        unread: quickFilterRef.current === 'unread' || undefined,
      })
    } finally {
      loadingRef.current = false
      setLoadingMore(false)
    }
  }, [setLoadingMore, loadConversations])

  // load conversations when any filter changes
  // debounce only while the user is actively typing in search
  useEffect(() => {
    const searchChanged = searchQuery !== prevSearchRef.current
    prevSearchRef.current = searchQuery

    const doLoad = () => {
      // flip loading true before clearing so the UI shows the skeleton
      // instead of the "All caught up!" empty state while refetching
      setInitialLoading(true)
      setConversations([])
      setHasMore(true)
      loadConversations({
        archived: showArchived || undefined,
        category: categoryFilter,
        domains: selectedDomains.length > 0 ? selectedDomains : undefined,
        folder,
        query: searchQuery || undefined,
        section: importanceSection,
        starred: quickFilter === 'starred' || undefined,
        unread: quickFilter === 'unread' || undefined,
      })
    }

    if (debounceRef.current) clearTimeout(debounceRef.current)

    if (searchChanged && searchQuery) {
      debounceRef.current = setTimeout(doLoad, 300)
    } else {
      doLoad()
    }

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [
    searchQuery,
    categoryFilter,
    selectedDomains,
    showArchived,
    folder,
    quickFilter,
    importanceSection,
    loadConversations,
    setConversations,
    setHasMore,
    setInitialLoading,
  ])

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

  const refreshConversations = useCallback(
    () =>
      loadConversations({
        archived: showArchived || undefined,
        category: categoryFilter,
        domains: selectedDomains.length > 0 ? selectedDomains : undefined,
        folder,
        query: searchQuery || undefined,
        section: importanceSection,
        starred: quickFilter === 'starred' || undefined,
        unread: quickFilter === 'unread' || undefined,
      }),
    [
      loadConversations,
      showArchived,
      categoryFilter,
      selectedDomains,
      folder,
      searchQuery,
      importanceSection,
      quickFilter,
    ]
  )

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
