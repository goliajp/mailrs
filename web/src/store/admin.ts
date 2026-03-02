import { atom } from 'jotai'

import type { AccountInfo, DomainInfo, QueueEntry } from '@/lib/types'

export const domainsAtom = atom<DomainInfo[]>([])
export const accountsAtom = atom<AccountInfo[]>([])
export const queueAtom = atom<QueueEntry[]>([])
