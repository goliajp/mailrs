import type { ComposerStart } from '@/components/composer/start'

import { useAtomValue, useSetAtom } from 'jotai'
import { X } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ComposerView } from '@/components/composer/composer'
import { useComposer } from '@/components/composer/use-composer'
import { useComposerAi } from '@/components/composer/use-composer-ai'
import { type Key, useT } from '@/i18n'
import {
  composeCarrySourceAtom,
  composeDraftSourceAtom,
  composeRedraftSourceAtom,
  composeReplySourceAtom,
  composingNewAtom,
} from '@/store/ui'

import { CarriedAttachments } from './carried-attachments'
import { carriedSelection } from './carried-model'

/**
 * The page shell: the composer filling the reading pane. It opens new, as a
 * reply from the thread's Reply button, from a saved draft, from a failed
 * send, or from a floating window that was expanded.
 */
export function NewConversation() {
  const t = useT()
  const setComposingNew = useSetAtom(composingNewAtom)
  const setReplySource = useSetAtom(composeReplySourceAtom)
  const setDraftSource = useSetAtom(composeDraftSourceAtom)
  const setRedraftSource = useSetAtom(composeRedraftSourceAtom)
  const setCarrySource = useSetAtom(composeCarrySourceAtom)
  const start = usePageStart()
  const redraft = start.kind === 'redraft' ? start.redraft : null
  // which carried attachments survive this edit; all of them to begin with
  const [kept, setKept] = useState<Set<number>>(
    () => new Set((redraft?.attachments ?? []).map((a) => a.index))
  )

  const close = () => {
    // what is typed is kept as a draft; the request outlives the unmount
    void c.saveNow()
    // with our history entry still on top, going back closes through popstate
    const state = window.history.state as null | { composerSentinel?: boolean }
    if (state?.composerSentinel) {
      window.history.back()
      return
    }
    setComposingNew(false)
    setReplySource(null)
  }

  const c = useComposer(start, { onSent: close, redraftKeep: carriedSelection(redraft, kept) })
  const ai = useComposerAi(c)

  // the sources are read once, at the start; the next compose starts clean
  useEffect(() => {
    return () => {
      setDraftSource(null)
      setRedraftSource(null)
      setCarrySource(null)
    }
  }, [setCarrySource, setDraftSource, setRedraftSource])

  // opening is an atom flip, not a navigation: push an entry so Back closes
  // the composer instead of leaving /mail
  useEffect(() => {
    window.history.pushState({ composerSentinel: true }, '')
    const onPop = () => {
      setComposingNew(false)
      setReplySource(null)
    }
    window.addEventListener('popstate', onPop)
    return () => window.removeEventListener('popstate', onPop)
  }, [setComposingNew, setReplySource])

  return (
    // bounded, not only flex-1: a flex child's min-height:auto would let a
    // long message push Send off the bottom of the window
    <div className="flex h-full min-h-0 flex-1 flex-col">
      <div className="border-border flex shrink-0 items-center justify-between border-b px-4 py-2">
        <span className="text-fg-muted text-xs font-medium">{t(headerLabel(start))}</span>
        <button
          aria-label={t('common.close')}
          className="text-fg-muted hover:bg-bg-secondary rounded-md p-1 transition-colors"
          onClick={close}
          type="button"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
      <ComposerView ai={ai} c={c} fieldsOpen focusTo onCancel={close}>
        <CarriedAttachments
          items={redraft?.attachments ?? []}
          kept={kept}
          onToggle={(index) => setKept(toggleIndex(kept, index))}
        />
      </ComposerView>
    </div>
  )
}

function headerLabel(s: ComposerStart): Key {
  if (s.kind === 'reply') return 'compose.replyHeader'
  return 'compose.newHeader'
}

/** Immutable set toggle — the codebase creates, it does not mutate. */
function toggleIndex(current: ReadonlySet<number>, index: number): Set<number> {
  const next = new Set(current)
  if (next.has(index)) {
    next.delete(index)
    return next
  }
  next.add(index)
  return next
}

/** The start, from whichever source opened the page; read once. */
function usePageStart(): ComposerStart {
  const reply = useAtomValue(composeReplySourceAtom)
  const draft = useAtomValue(composeDraftSourceAtom)
  const redraft = useAtomValue(composeRedraftSourceAtom)
  const carry = useAtomValue(composeCarrySourceAtom)
  const [start] = useState<ComposerStart>(() => {
    if (carry) return { carry, kind: 'carry' }
    if (draft) return { draft, kind: 'draft' }
    if (redraft) return { kind: 'redraft', redraft }
    if (reply) return { kind: 'reply', reply }
    return { kind: 'new' }
  })
  return start
}
