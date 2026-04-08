import type { ConversationSummary, ThreadMessage } from '@/lib/types'

import { atom } from 'jotai'

export const conversationsAtom = atom<ConversationSummary[]>([])
export const unreadCountAtom = atom((get) => {
  const conversations = get(conversationsAtom)
  return conversations.reduce((sum, c) => sum + c.unread_count, 0)
})
export const selectedThreadIdAtom = atom<null | string>(null)
export const threadMessagesAtom = atom<ThreadMessage[]>([])
export const composingNewAtom = atom(false)
export const searchQueryAtom = atom('')
export const hasMoreAtom = atom(true)
export const loadingMoreAtom = atom(false)
export const categoryFilterAtom = atom<null | string>(null)
export const selectedDomainsAtom = atom<string[]>([])
export const initialLoadingAtom = atom(true)
export type MobileView = 'conversation' | 'list' | 'reply' | 'thread'
export const mobileViewAtom = atom<MobileView>('list')

export type SortOrder = 'newest' | 'oldest' | 'unread'
export const sortOrderAtom = atom<SortOrder>('newest')

// batch selection mode
export const batchModeAtom = atom(false)
export const selectedThreadIdsAtom = atom<Set<string>>(new Set<string>())

// mailbox folder filter (null = INBOX default, 'Sent' = sent folder)
export type MailFolder = 'Drafts' | 'Sent' | 'Trash' | null
export const folderAtom = atom<MailFolder>(null)

// archived view toggle
export const showArchivedAtom = atom(false)

// supermode: mark read across all domain accounts
export const crossAccountReadAtom = atom(false)

// importance section filter: null = all, or 'action' | 'important' | 'other'
export type ImportanceSection = 'action' | 'important' | 'other' | null
export const importanceSectionAtom = atom<ImportanceSection>(null)

// quick filter
export type QuickFilter = 'all' | 'attachment' | 'starred' | 'unread'
export const quickFilterAtom = atom<QuickFilter>('all')

// keyboard shortcuts dialog
export const shortcutsDialogOpenAtom = atom(false)

// visible conversation ids in display order (synced from conversation-list)
export const visibleConversationIdsAtom = atom<string[]>([])

// websocket connection status
export type ConnectionStatus = 'connected' | 'connecting' | 'offline'
export const connectionStatusAtom = atom<ConnectionStatus>('connecting')

// mobile thread view: toggle between email content and conversation timeline
export type MobileThreadTab = 'content' | 'conversation'
export const mobileThreadTabAtom = atom<MobileThreadTab>('content')

// mobile full-screen reply composer
export const mobileReplyOpenAtom = atom(false)
