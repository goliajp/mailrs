import type { ConversationSummary } from '@/lib/types'

import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef } from 'react'

import { ConversationList } from '@/components/conversation-list'
import { KeyboardShortcutsDialog } from '@/components/keyboard-shortcuts-dialog'
import { NewConversation } from '@/components/new-conversation'
import { ThreadView } from '@/components/thread-view'
import { useKeyboardNav } from '@/hooks/use-keyboard-nav'
import { useMailEvents } from '@/hooks/use-mail-events'
import { Panel, PanelRow } from '@/layouts/shell'
import { fetchJson } from '@/lib/api'
import { authAtom } from '@/store/auth'
import {
  categoryFilterAtom,
  composingNewAtom,
  conversationsAtom,
  folderAtom,
  hasMoreAtom,
  initialLoadingAtom,
  loadingMoreAtom,
  mobileViewAtom,
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
  const [selectedThreadId, setSelectedThreadId] = useAtom(selectedThreadIdAtom)
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(null)
  const firstLoadDone = useRef(false)

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

  // request notification permission
  useEffect(() => {
    if (
      typeof Notification !== 'undefined' &&
      Notification.permission === 'default'
    ) {
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
    }) => {
      const {
        archived,
        before,
        category,
        domains,
        folder: f,
        query,
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
      if (domains && domains.length > 0)
        path += `&domains=${encodeURIComponent(domains.join(','))}`
      if (archived) path += '&archived=true'
      if (f) path += `&folder=${encodeURIComponent(f)}`
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
        if (!firstLoadDone.current) {
          firstLoadDone.current = true
          setInitialLoading(false)
        }
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
      })
    } finally {
      loadingRef.current = false
      setLoadingMore(false)
    }
  }, [setLoadingMore, loadConversations])

  // initial load + react to filter/search/domain/archived/folder changes
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      setHasMore(true)
      loadConversations({
        archived: showArchived || undefined,
        category: categoryFilter,
        domains: selectedDomains.length > 0 ? selectedDomains : undefined,
        folder,
        query: searchQuery || undefined,
      })
    }, 300)
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [
    searchQuery,
    categoryFilter,
    selectedDomains,
    showArchived,
    folder,
    loadConversations,
    setHasMore,
  ])

  // auto-select first conversation when list loads and nothing is selected
  useEffect(() => {
    if (!selectedThreadId && !composingNew && conversations.length > 0) {
      setSelectedThreadId(conversations[0].thread_id)
    }
  }, [conversations, selectedThreadId, composingNew, setSelectedThreadId])

  // mobile: show list or thread exclusively; desktop: show both side by side
  const showList = mobileView === 'list'
  const showThread = mobileView === 'thread'

  return (
    <PanelRow>
      <Panel className={showThread ? 'hidden md:flex' : ''} width={480}>
        <ConversationList
          onLoadMore={loadMore}
          onSelectConversation={() => setMobileView('thread')}
        />
      </Panel>

      <PanelRow className={showList ? 'hidden md:flex' : ''}>
        {composingNew ? (
          <Panel>
            <NewConversation />
          </Panel>
        ) : (
          <ThreadView onBack={() => setMobileView('list')} />
        )}
      </PanelRow>

      <KeyboardShortcutsDialog
        onClose={() => setShortcutsOpen(false)}
        open={shortcutsOpen}
      />
    </PanelRow>
  )
}
