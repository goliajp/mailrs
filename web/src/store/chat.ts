import { atom } from 'jotai'

import type { ConversationSummary, ThreadMessage } from '@/lib/types'

export const conversationsAtom = atom<ConversationSummary[]>([])
export const unreadCountAtom = atom((get) => {
  const conversations = get(conversationsAtom)
  return conversations.reduce((sum, c) => sum + c.unread_count, 0)
})
export const selectedThreadIdAtom = atom<string | null>(null)
export const threadMessagesAtom = atom<ThreadMessage[]>([])
export const composingNewAtom = atom(false)
export const searchQueryAtom = atom('')
export const hasMoreAtom = atom(true)
export const loadingMoreAtom = atom(false)
export const categoryFilterAtom = atom<string | null>(null)
export const selectedDomainsAtom = atom<string[]>([])
export const initialLoadingAtom = atom(true)
export const mobileViewAtom = atom<'list' | 'thread'>('list')

export type SortOrder = 'newest' | 'oldest' | 'unread'
export const sortOrderAtom = atom<SortOrder>('newest')

// batch selection mode
export const batchModeAtom = atom(false)
export const selectedThreadIdsAtom = atom<Set<string>>(new Set<string>())

// archived view toggle
export const showArchivedAtom = atom(false)

// supermode: mark read across all domain accounts
export const crossAccountReadAtom = atom(false)

// importance section filter: null = all, or 'action' | 'important' | 'other'
export type ImportanceSection = 'action' | 'important' | 'other' | null
export const importanceSectionAtom = atom<ImportanceSection>(null)

// keyboard shortcuts dialog
export const shortcutsDialogOpenAtom = atom(false)

// visible conversation ids in display order (synced from conversation-list)
export const visibleConversationIdsAtom = atom<string[]>([])
