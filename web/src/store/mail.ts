import { atom } from 'jotai'

import type { FolderInfo, MessageDetail, MessageSummary } from '@/lib/types'

export type ComposeState = {
  to: string
  cc: string
  bcc: string
  subject: string
  body: string
  replyTo?: string
}

export const currentUserAtom = atom('')
export const foldersAtom = atom<FolderInfo[]>([])
export const messagesAtom = atom<MessageSummary[]>([])
export const selectedFolderAtom = atom('INBOX')
export const selectedMessageUidAtom = atom<number | null>(null)
export const selectedMessageDetailAtom = atom<MessageDetail | null>(null)
export const composingAtom = atom<ComposeState | null>(null)
