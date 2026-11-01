import type { Composer } from './use-composer'

import { toast } from '@goliapkg/gds'
import { useRef, useState } from 'react'

import { useCurrentThreadMessages } from '@/hooks/use-current-list'
import { useT } from '@/i18n'
import { wireGenerateSubject, wirePolishText, wireReplySuggest } from '@/wire/endpoints/ai'

export type ComposerAi = ReturnType<typeof useComposerAi>

/** The assistant's part of a compose session: polish, suggested replies, a subject. */
export function useComposerAi(c: Composer) {
  const t = useT()
  const [polishing, setPolishing] = useState(false)
  const [tone, setTone] = useState('professional')
  const [suggesting, setSuggesting] = useState(false)
  const [suggestions, setSuggestions] = useState<string[]>([])
  const [generatingSubject, setGeneratingSubject] = useState(false)
  const beforePolish = useRef<null | string>(null)
  // the open thread's messages, read through a ref so a refetch does not
  // re-render the editor
  const threadMessages = useCurrentThreadMessages()
  const messagesRef = useRef(threadMessages)
  messagesRef.current = threadMessages

  const polish = async () => {
    const handle = c.editorRef.current
    if (!handle) return
    const text = handle.getMarkdown()
    if (!text.trim()) return
    beforePolish.current = text
    setPolishing(true)
    try {
      const result = await wirePolishText(text, tone)
      if (result.success && result.polished) {
        handle.setMarkdown(result.polished)
        toast.success(t('compose.polished'), {
          action: {
            label: t('common.undo'),
            onClick: () => {
              if (beforePolish.current === null) return
              handle.setMarkdown(beforePolish.current)
              beforePolish.current = null
            },
          },
          duration: 8000,
        })
      } else {
        toast.error(result.message ?? t('compose.polishFailed'))
        beforePolish.current = null
      }
    } catch {
      toast.error(t('compose.aiUnavailable'))
      beforePolish.current = null
    } finally {
      setPolishing(false)
    }
  }

  const suggest = async () => {
    if (c.start.kind !== 'inline') return
    const ctx = c.start.ctx
    setSuggesting(true)
    try {
      // up to three earlier messages, the newest one excluded
      const prior = messagesRef.current.slice(0, -1).slice(-3)
      const threadContext = prior
        .map((m) => `From: ${m.sender}\n${m.clean_text || m.text_body || ''}`)
        .join('\n---\n')
      const result = await wireReplySuggest({
        original_body: ctx.originalBody,
        original_sender: ctx.originalFrom,
        original_subject: ctx.subject,
        thread_context: threadContext || undefined,
      })
      if (result.success && result.suggestions.length > 0) setSuggestions(result.suggestions)
      else toast.error(result.message ?? t('compose.noSuggestions'))
    } catch {
      toast.error(t('compose.aiUnavailable'))
    } finally {
      setSuggesting(false)
    }
  }

  const applySuggestion = (text: string) => {
    c.editorRef.current?.setMarkdown(text)
    setSuggestions([])
    toast.success(t('compose.suggestionApplied'))
  }

  const generateSubject = async () => {
    const content = c.editorRef.current?.getContent()
    if (!content?.compose.text.trim()) {
      toast.error(t('compose.writeFirst'))
      return
    }
    setGeneratingSubject(true)
    try {
      const result = await wireGenerateSubject({
        body: content.compose.text,
        context: toContext(c.to),
      })
      if (result.success && result.subject) {
        c.setSubject(result.subject)
        toast.success(t('compose.subjectGenerated'))
      } else {
        toast.error(result.message ?? t('common.failed'))
      }
    } catch {
      toast.error(t('compose.aiUnavailable'))
    } finally {
      setGeneratingSubject(false)
    }
  }

  return {
    applySuggestion,
    canSuggest: c.start.kind === 'inline' && c.start.mode !== 'forward',
    generateSubject,
    generatingSubject,
    polish,
    polishing,
    setTone,
    suggest,
    suggesting,
    suggestions,
    tone,
    dismissSuggestions: () => setSuggestions([]),
  }
}

function toContext(to: string): string | undefined {
  if (!to) return undefined
  return `To: ${to}`
}
