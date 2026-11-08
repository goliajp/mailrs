import type { Composer } from './use-composer'
import type { ComposerAi } from './use-composer-ai'

import { Clock, Eye, Loader2, Send, X } from 'lucide-react'
import { useState } from 'react'

import { currentLocale, type Key, useT } from '@/i18n'
import { sendLabel } from '@/lib/compose-labels'

const TONES: readonly { label: Key; value: string }[] = [
  { label: 'compose.tone.professional', value: 'professional' },
  { label: 'compose.tone.casual', value: 'casual' },
  { label: 'compose.tone.formal', value: 'formal' },
  { label: 'compose.tone.friendly', value: 'friendly' },
  { label: 'compose.tone.concise', value: 'concise' },
]

const QUIET =
  'text-accent hover:bg-accent/10 disabled:text-fg-muted flex h-8 shrink-0 items-center rounded-md px-2 text-xs whitespace-nowrap transition-colors disabled:cursor-not-allowed disabled:opacity-50'

/** Send, schedule, template, the assistant and preview: the same bar in every shell. */
export function ComposerActions({
  ai,
  c,
  compact,
  onCancel,
  onPreview,
}: {
  ai: ComposerAi
  c: Composer
  /** Tighter padding, for the floating window. */
  compact?: boolean
  /** Shells that can be closed get a Cancel. */
  onCancel?: () => void
  onPreview: () => void
}) {
  const t = useT()
  const [picking, setPicking] = useState(false)
  const busy = c.sending
  return (
    <div className={barClass(compact)}>
      {c.scheduledAt && (
        <span className="bg-accent/10 text-accent inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs">
          {new Date(c.scheduledAt).toLocaleString(currentLocale(), {
            day: 'numeric',
            hour: 'numeric',
            minute: '2-digit',
            month: 'short',
          })}
          <button
            aria-label={t('compose.clearSchedule')}
            className="rounded-full p-0.5 hover:opacity-70"
            onClick={() => {
              c.setScheduledAt('')
              setPicking(false)
            }}
            type="button"
          >
            <X className="h-3 w-3" />
          </button>
        </span>
      )}
      <button
        className="bg-accent hover:bg-accent-hover text-accent-fg flex h-8 shrink-0 items-center gap-1.5 rounded-md px-4 text-sm font-medium whitespace-nowrap transition-all hover:shadow-md active:scale-95 disabled:cursor-not-allowed disabled:opacity-50"
        disabled={busy}
        onClick={() => void c.send()}
        type="button"
      >
        <Send className="h-3.5 w-3.5" />
        {t(sendLabel(busy))}
      </button>
      <button
        aria-label={t('compose.schedule')}
        aria-pressed={picking}
        className={scheduleClass(picking)}
        disabled={busy}
        onClick={() => setPicking((v) => !v)}
        title={t('compose.schedule')}
        type="button"
      >
        <Clock className="h-4 w-4" />
      </button>
      {picking && (
        <input
          aria-label={t('compose.scheduleTime')}
          className="border-border bg-bg-secondary text-fg focus:border-accent h-8 w-44 max-w-full shrink-0 rounded-md border px-2 text-xs outline-none"
          min={localDatetimeMin()}
          onChange={(e) => c.setScheduledAt(e.target.value)}
          type="datetime-local"
          value={c.scheduledAt}
        />
      )}

      <div className="bg-border mx-0.5 h-4 w-px" />

      <div className="relative flex shrink-0">
        <button
          aria-label={t('compose.polishWithTone', { tone: toneName(ai.tone, t) })}
          className={QUIET}
          disabled={ai.polishing || busy}
          onClick={() => void ai.polish()}
          title={t('compose.polishWithTone', { tone: toneName(ai.tone, t) })}
          type="button"
        >
          {ai.polishing && <Loader2 className="h-3 w-3 animate-spin" />}
          {!ai.polishing && t('compose.polish')}
        </button>
        <select
          aria-label={t('compose.polishTone')}
          className="text-accent hover:bg-accent/10 text-tiny h-8 appearance-none rounded-md bg-transparent px-1 outline-none disabled:cursor-not-allowed disabled:opacity-50"
          disabled={ai.polishing || busy}
          onChange={(e) => ai.setTone(e.target.value)}
          value={ai.tone}
        >
          {TONES.map((o) => (
            <option key={o.value} value={o.value}>
              {t(o.label)}
            </option>
          ))}
        </select>
      </div>

      {ai.canSuggest && (
        <button
          aria-label={t('compose.suggestReplies')}
          className={QUIET}
          disabled={ai.suggesting || busy}
          onClick={() => void ai.suggest()}
          title={t('compose.suggestTitle')}
          type="button"
        >
          {ai.suggesting && <Loader2 className="h-3 w-3 animate-spin" />}
          {!ai.suggesting && t('compose.suggest')}
        </button>
      )}

      {c.templates.length > 0 && (
        <select
          aria-label={t('compose.applyTemplate')}
          className="border-border bg-bg-secondary text-fg-secondary h-8 max-w-32 rounded-md border px-2 text-xs disabled:cursor-not-allowed disabled:opacity-50"
          defaultValue=""
          disabled={busy}
          onChange={(e) => {
            const picked = c.templates.find((tpl) => tpl.id === Number(e.target.value))
            if (picked) c.applyTemplate(picked)
            e.target.value = ''
          }}
        >
          <option disabled value="">
            {t('compose.templates')}
          </option>
          {c.templates.map((tpl) => (
            <option key={tpl.id} value={tpl.id}>
              {tpl.name}
            </option>
          ))}
        </select>
      )}

      <div className="flex-1" />
      <button
        className="border-border text-fg hover:bg-bg-tertiary flex h-8 shrink-0 items-center gap-1.5 rounded-md border px-3 text-xs font-medium whitespace-nowrap transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        disabled={busy}
        onClick={onPreview}
        title={t('compose.previewTitle')}
        type="button"
      >
        <Eye className="h-3.5 w-3.5" />
        {t('compose.preview')}
      </button>
      {onCancel && (
        <button
          className="text-fg-muted hover:bg-bg-secondary flex h-8 shrink-0 items-center rounded-md px-3 text-xs whitespace-nowrap transition-colors disabled:cursor-not-allowed disabled:opacity-50"
          disabled={busy}
          onClick={onCancel}
          type="button"
        >
          {t('common.cancel')}
        </button>
      )}
    </div>
  )
}

function barClass(compact: boolean | undefined): string {
  const base = 'border-border flex shrink-0 items-center gap-1 border-t py-2 select-none'
  // wraps in every shell: a row that scrolled sideways hid Preview and
  // Cancel past the floating window's edge with no sign they were there
  if (compact) return `${base} flex-wrap px-3`
  return `${base} flex-wrap px-4`
}

function localDatetimeMin(): string {
  const d = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}

function scheduleClass(on: boolean): string {
  const base =
    'flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-colors disabled:cursor-not-allowed disabled:opacity-50'
  if (on) return `${base} bg-accent/10 text-accent`
  return `${base} text-fg-muted hover:bg-bg-secondary`
}

function toneName(tone: string, t: ReturnType<typeof useT>): string {
  const option = TONES.find((o) => o.value === tone)
  if (option) return t(option.label)
  return tone
}
