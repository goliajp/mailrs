import type { Composer as ComposerState } from './use-composer'
import type { ComposerAi } from './use-composer-ai'

import { lazy, type ReactNode, Suspense, useState } from 'react'

import { AutosaveWarning } from '@/components/autosave-warning'
import { ComposerSkeleton } from '@/components/composer-skeleton'
import { PreviewDialog } from '@/components/reply-box/preview-dialog'
import { SuggestionsRow } from '@/components/reply-box/suggestions-row'
import { type Key, useT } from '@/i18n'

import { ComposerActions } from './composer-actions'
import { ComposerFields } from './composer-fields'
import { quoteOf } from './quote'
import { isReplyLike } from './start'

const StructuredCompose = lazy(() =>
  import('@/components/structured-compose').then((m) => ({ default: m.StructuredCompose }))
)

export type ComposerLayout = 'floating' | 'inline' | 'page'

/**
 * The composer every shell shows. The shell decides the frame around it —
 * a page header, a floating window, the inline reply's mode toggle — and
 * nothing about what can be written or how it is sent.
 */
export function ComposerView({
  ai,
  c,
  children,
  compact,
  fieldsOpen,
  focusTo,
  onCancel,
}: {
  ai: ComposerAi
  c: ComposerState
  /** Between the fields and the editor: what only one shell has, like carried attachments. */
  children?: ReactNode
  /** The narrow shells: the action bar stays one row. */
  compact?: boolean
  /** The inline reply folds the fields away until asked. */
  fieldsOpen: boolean
  focusTo: boolean
  onCancel?: () => void
}) {
  const t = useT()
  const [previewHtml, setPreviewHtml] = useState<null | string>(null)
  const quote = quoteOf(c.start, t)
  return (
    <>
      {c.error && (
        <div
          className="bg-danger/10 text-danger mx-4 mt-2 shrink-0 rounded-md px-3 py-1.5 text-xs"
          role="alert"
        >
          {c.error}
        </div>
      )}
      {fieldsOpen && <ComposerFields ai={ai} c={c} focusTo={focusTo} />}
      {children}
      <SuggestionsRow
        onApply={ai.applySuggestion}
        onDismiss={ai.dismissSuggestions}
        suggestions={ai.suggestions}
      />
      {/* the scroll lives on the part that grows: the fields above and the
          bar below stay put, so Send is reachable however long the message */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <Suspense fallback={<ComposerSkeleton />}>
          <StructuredCompose
            disabled={c.sending}
            mode={editorMode(c)}
            onSubmit={() => void c.send()}
            placeholder={t(placeholderOf(c))}
            quotedHeader={quote.header}
            quotedHeaderHtml={quote.headerHtml}
            quotedHtml={quote.html}
            ref={c.editorRef}
            signature={c.signature}
            signatureEnabled={c.signature.trim().length > 0}
          />
        </Suspense>
      </div>
      <AutosaveWarning error={c.autosave.lastError} show={c.autosave.shouldWarn} />
      <ComposerActions
        ai={ai}
        c={c}
        compact={compact}
        onCancel={onCancel}
        onPreview={() => setPreviewHtml(c.editorRef.current?.getContent().fullHtml ?? '')}
      />
      {previewHtml !== null && (
        <PreviewDialog
          html={previewHtml}
          onClose={() => setPreviewHtml(null)}
          onSend={() => {
            setPreviewHtml(null)
            void c.send()
          }}
          sending={c.sending}
        />
      )}
    </>
  )
}

function editorMode(c: ComposerState): 'forward' | 'new' | 'reply' {
  if (c.mode === 'forward') return 'forward'
  if (isReplyLike(c.start)) return 'reply'
  return 'new'
}

function placeholderOf(c: ComposerState): Key {
  if (c.mode === 'forward') return 'compose.forwardPlaceholder'
  if (isReplyLike(c.start)) return 'compose.replyPlaceholder'
  return 'compose.bodyPlaceholder'
}
