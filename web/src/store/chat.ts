import { atom } from 'jotai'

import type { ConversationSummary, ThreadMessage } from '@/lib/types'

export const conversationsAtom = atom<ConversationSummary[]>([])
export const selectedThreadIdAtom = atom<string | null>(null)
export const threadMessagesAtom = atom<ThreadMessage[]>([])
export const composingNewAtom = atom(false)
export const searchQueryAtom = atom('')
export const hasMoreAtom = atom(true)
export const loadingMoreAtom = atom(false)
export const categoryFilterAtom = atom<string | null>(null)
