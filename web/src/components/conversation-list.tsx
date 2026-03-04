import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect, useRef, useState } from 'react'

import { CategoryBadge } from '@/components/category-badge'
import { fetchJson } from '@/lib/api'
import { avatarColor, avatarInitial, extractName } from '@/lib/avatar'
import { formatDate } from '@/lib/format'
import type { CategoryCount, ConversationSummary } from '@/lib/types'
import {
  categoryFilterAtom,
  composingNewAtom,
  conversationsAtom,
  hasMoreAtom,
  loadingMoreAtom,
  searchQueryAtom,
  selectedThreadIdAtom,
} from '@/store/chat'

function ConversationItem({
  convo,
  selected,
  onSelect,
}: {
  convo: ConversationSummary
  selected: boolean
  onSelect: () => void
}) {
  const firstParticipant = convo.participants[0] ?? ''
  const name = extractName(firstParticipant)
  const initial = avatarInitial(firstParticipant)
  const color = avatarColor(firstParticipant)
  const hasUnread = convo.unread_count > 0

  return (
    <button
      onClick={onSelect}
      className={`flex w-full items-start gap-3 px-4 py-3 text-left transition-colors ${
        selected
          ? 'bg-zinc-100 dark:bg-zinc-800'
          : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50'
      }`}
    >
      <div
        className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-sm font-medium text-white ${color}`}
      >
        {initial}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-2">
          <span
            className={`truncate text-sm ${hasUnread ? 'font-semibold text-zinc-900 dark:text-zinc-100' : 'text-zinc-700 dark:text-zinc-300'}`}
          >
            {name}
            {convo.participants.length > 1 && (
              <span className="text-zinc-400">
                {' '}
                +{convo.participants.length - 1}
              </span>
            )}
          </span>
          <span className="shrink-0 text-xs text-zinc-400">
            {formatDate(convo.last_date)}
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          <p
            className={`min-w-0 flex-1 truncate text-sm ${hasUnread ? 'font-medium text-zinc-800 dark:text-zinc-200' : 'text-zinc-500 dark:text-zinc-400'}`}
          >
            {convo.subject || '(no subject)'}
          </p>
          {convo.category && convo.category !== 'general' && (
            <CategoryBadge category={convo.category} />
          )}
          {hasUnread && (
            <span className="flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-blue-500 px-1.5 text-xs font-medium text-white">
              {convo.unread_count}
            </span>
          )}
        </div>
      </div>
    </button>
  )
}

function CategoryChips() {
  const [categories, setCategories] = useState<CategoryCount[]>([])
  const [activeCategory, setActiveCategory] = useAtom(categoryFilterAtom)

  useEffect(() => {
    fetchJson<CategoryCount[]>('/conversations/categories').then(
      (data) => setCategories(data),
      () => {}
    )
  }, [])

  if (categories.length === 0) return null

  return (
    <div className="flex gap-1.5 overflow-x-auto border-b border-zinc-200 px-3 py-2 dark:border-zinc-800">
      <button
        onClick={() => setActiveCategory(null)}
        className={`shrink-0 rounded-full px-2.5 py-0.5 text-xs font-medium transition-colors ${
          activeCategory === null
            ? 'bg-zinc-800 text-white dark:bg-zinc-200 dark:text-zinc-900'
            : 'bg-zinc-100 text-zinc-600 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700'
        }`}
      >
        All
      </button>
      {categories.map((cat) => (
        <button
          key={cat.category}
          onClick={() =>
            setActiveCategory(
              activeCategory === cat.category ? null : cat.category
            )
          }
          className={`shrink-0 rounded-full px-2.5 py-0.5 text-xs font-medium capitalize transition-colors ${
            activeCategory === cat.category
              ? 'bg-zinc-800 text-white dark:bg-zinc-200 dark:text-zinc-900'
              : 'bg-zinc-100 text-zinc-600 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700'
          }`}
        >
          {cat.category} ({cat.count})
        </button>
      ))}
    </div>
  )
}

export function ConversationList({ onLoadMore }: { onLoadMore: () => void }) {
  const conversations = useAtomValue(conversationsAtom)
  const [selectedId, setSelectedId] = useAtom(selectedThreadIdAtom)
  const setComposingNew = useSetAtom(composingNewAtom)
  const [searchQuery, setSearchQuery] = useAtom(searchQueryAtom)
  const hasMore = useAtomValue(hasMoreAtom)
  const loadingMore = useAtomValue(loadingMoreAtom)

  // refs to avoid stale closures in observer callback
  const onLoadMoreRef = useRef(onLoadMore)
  onLoadMoreRef.current = onLoadMore
  const loadingRef = useRef(loadingMore)
  loadingRef.current = loadingMore

  // observer ref to clean up when sentinel unmounts
  const observerRef = useRef<IntersectionObserver | null>(null)
  const scrollContainerRef = useRef<HTMLDivElement>(null)

  // callback ref: called when sentinel mounts/unmounts
  const sentinelCallback = useCallback((node: HTMLDivElement | null) => {
    // disconnect old observer
    if (observerRef.current) {
      observerRef.current.disconnect()
      observerRef.current = null
    }

    if (!node) return

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !loadingRef.current) {
          onLoadMoreRef.current()
        }
      },
      {
        root: scrollContainerRef.current,
        rootMargin: '300px',
      }
    )
    observer.observe(node)
    observerRef.current = observer
  }, [])

  // cleanup on unmount
  useEffect(() => {
    return () => {
      observerRef.current?.disconnect()
    }
  }, [])

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-zinc-200 p-3 dark:border-zinc-800">
        <div className="relative flex-1">
          <svg
            className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-400"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
          >
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.3-4.3" />
          </svg>
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search..."
            className="w-full rounded-md border border-zinc-200 bg-zinc-50 py-1.5 pl-9 pr-3 text-sm text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-400 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:border-zinc-500"
          />
        </div>
        <button
          onClick={() => {
            setComposingNew(true)
            setSelectedId(null)
          }}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-zinc-500 transition-colors hover:bg-zinc-100 dark:hover:bg-zinc-800"
          title="New conversation"
        >
          <svg
            className="h-5 w-5"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M16.862 4.487l1.687-1.688a1.875 1.875 0 112.652 2.652L6.832 19.82a4.5 4.5 0 01-1.897 1.13l-2.685.8.8-2.685a4.5 4.5 0 011.13-1.897L16.863 4.487zm0 0L19.5 7.125"
            />
          </svg>
        </button>
      </div>

      <CategoryChips />

      <div ref={scrollContainerRef} className="flex-1 overflow-y-auto">
        {conversations.length === 0 ? (
          <div className="p-6 text-center text-sm text-zinc-400">
            No conversations
          </div>
        ) : (
          conversations.map((c) => (
            <ConversationItem
              key={c.thread_id}
              convo={c}
              selected={selectedId === c.thread_id}
              onSelect={() => {
                setSelectedId(c.thread_id)
                setComposingNew(false)
              }}
            />
          ))
        )}

        {/* infinite scroll sentinel */}
        {hasMore && conversations.length > 0 && (
          <div ref={sentinelCallback} className="flex justify-center py-4">
            {loadingMore && (
              <div className="h-5 w-5 animate-spin rounded-full border-2 border-zinc-300 border-t-zinc-600 dark:border-zinc-600 dark:border-t-zinc-300" />
            )}
          </div>
        )}

        {!hasMore && conversations.length > 0 && (
          <div className="py-3 text-center text-xs text-zinc-400">
            No more conversations
          </div>
        )}
      </div>
    </div>
  )
}
