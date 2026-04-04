import type { AccountInfo, AliasInfo, DomainInfo, QueueEntry } from '@/lib/types'

import { atom } from 'jotai'

export const domainsAtom = atom<DomainInfo[]>([])
export const accountsAtom = atom<AccountInfo[]>([])
export const aliasesAtom = atom<AliasInfo[]>([])
export const queueAtom = atom<QueueEntry[]>([])
