import type { ReplyMode } from './types'

import { Eye, Loader2, Send } from 'lucide-react'

import { type Key, useT } from '@/i18n'
import { sendLabel } from '@/lib/compose-labels'

const TONE_OPTIONS: readonly { label: Key; value: string }[] = [
  { label: 'compose.tone.professional', value: 'professional' },
  { label: 'compose.tone.casual', value: 'casual' },
  { label: 'compose.tone.formal', value: 'formal' },
  { label: 'compose.tone.friendly', value: 'friendly' },
  { label: 'compose.tone.concise', value: 'concise' },
]

type ActionBarProps = {
  mode: ReplyMode
  onPolish: () => void
  onPreview: () => void
  onSend: () => void
  onSuggest: () => void
  onToneChange: (tone: string) => void
  polishing: boolean
  sending: boolean
  suggesting: boolean
  tone: string
}

export function ActionBar({
  mode,
  onPolish,
  onPreview,
  onSend,
  onSuggest,
  onToneChange,
  polishing,
  sending,
  suggesting,
  tone,
}: ActionBarProps) {
  const t = useT()
  const toneName = toneLabel(tone, t)
  return (
    <div className="border-border flex shrink-0 flex-wrap items-center gap-1 border-t px-4 py-2 select-none">
      {mode !== 'forward' && (
        <button
          aria-label={t('compose.suggestReplies')}
          className="text-accent hover:bg-accent/10 disabled:text-fg-muted flex h-8 shrink-0 items-center rounded-md px-2 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50"
          disabled={suggesting || sending}
          onClick={onSuggest}
          title={t('compose.suggestTitle')}
          type="button"
        >
          {suggesting ? <Loader2 className="h-3 w-3 animate-spin" /> : t('compose.suggest')}
        </button>
      )}

      <div className="relative flex shrink-0">
        <button
          aria-label={t('compose.polishWithTone', { tone: toneName })}
          className="text-accent hover:bg-accent/10 disabled:text-fg-muted flex h-8 items-center rounded-l-md px-2 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50"
          disabled={polishing || sending}
          onClick={onPolish}
          title={t('compose.polishWithTone', { tone: toneName })}
          type="button"
        >
          {polishing ? <Loader2 className="h-3 w-3 animate-spin" /> : t('compose.polish')}
        </button>
        <select
          aria-label={t('compose.polishTone')}
          className="border-border text-accent hover:bg-accent/10 text-tiny h-8 appearance-none rounded-r-md border-l bg-transparent px-1 outline-none disabled:cursor-not-allowed disabled:opacity-50"
          disabled={polishing || sending}
          onChange={(e) => onToneChange(e.target.value)}
          value={tone}
        >
          {TONE_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {t(opt.label)}
            </option>
          ))}
        </select>
      </div>

      <div className="flex-1" />

      <button
        className="border-border text-fg hover:bg-bg-tertiary flex h-8 shrink-0 items-center gap-1.5 rounded-md border px-3 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        disabled={sending}
        onClick={onPreview}
        title={t('compose.previewTitle')}
        type="button"
      >
        <Eye className="h-3.5 w-3.5" />
        {t('compose.preview')}
      </button>

      <button
        className="bg-accent hover:bg-accent-hover flex h-8 shrink-0 items-center gap-1.5 rounded-md px-3 text-xs font-medium text-white transition-all hover:shadow-md active:scale-95 disabled:cursor-not-allowed disabled:opacity-50"
        disabled={sending}
        onClick={onSend}
        type="button"
      >
        <Send className="h-3.5 w-3.5" />
        {t(sendLabel(sending))}
      </button>
    </div>
  )
}

function toneLabel(tone: string, t: (key: Key) => string): string {
  const option = TONE_OPTIONS.find((o) => o.value === tone)
  if (option) return t(option.label)
  return tone
}
