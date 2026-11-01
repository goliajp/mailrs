import type { MailListId } from '@/lib/mail-lists'

import { useAtomValue, useSetAtom } from 'jotai'

import {
  activeListAtom,
  categoryFilterAtom,
  importanceSectionAtom,
  selectMailListAtom,
} from '@/store/ui'

/**
 * Opens a mail list, dropping the refinements stacked on the old one: a
 * category chosen inside Inbox is not a question about Junk, and leaving it
 * applied is how a list could open empty for no visible reason. The tab
 * chips and the folder column both open lists through this.
 */
export function useOpenList(): (id: MailListId) => void {
  const active = useAtomValue(activeListAtom)
  const select = useSetAtom(selectMailListAtom)
  const setCategory = useSetAtom(categoryFilterAtom)
  const setSection = useSetAtom(importanceSectionAtom)
  return (id) => {
    if (id === active) return
    setCategory(null)
    setSection(null)
    select(id)
  }
}
