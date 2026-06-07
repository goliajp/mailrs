import type { TemplateInfo } from './types'

import { Clock, Loader2, Send, X } from 'lucide-react'

type ActionBarProps = {
  onCancel: () => void
  onPolish: () => void
  onScheduleChange: (value: string) => void
  onScheduleClear: () => void
  onSend: () => void
  onTemplateSelect: (template: TemplateInfo) => void
  onToggleSchedulePicker: () => void
  polishing: boolean
  scheduledAt: string
  sending: boolean
  showSchedulePicker: boolean
  templates: TemplateInfo[]
}

export function ActionBar({
  onCancel,
  onPolish,
  onScheduleChange,
  onScheduleClear,
  onSend,
  onTemplateSelect,
  onToggleSchedulePicker,
  polishing,
  scheduledAt,
  sending,
  showSchedulePicker,
  templates,
}: ActionBarProps) {
  return (
    <div className="border-border flex shrink-0 flex-wrap items-center gap-1 border-t px-4 py-2 select-none">
      {scheduledAt && (
        <span className="bg-accent/10 text-accent inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs">
          {new Date(scheduledAt).toLocaleString(undefined, {
            day: 'numeric',
            hour: 'numeric',
            minute: '2-digit',
            month: 'short',
          })}
          <button
            aria-label="Clear schedule"
            className="rounded-full p-0.5 hover:opacity-70"
            onClick={onScheduleClear}
            type="button"
          >
            <X className="h-3 w-3" />
          </button>
        </span>
      )}
      <button
        className="bg-accent hover:bg-accent-hover flex h-8 shrink-0 items-center gap-1.5 rounded-md px-4 text-sm font-medium text-white transition-all hover:shadow-md active:scale-95 disabled:cursor-not-allowed disabled:opacity-50"
        disabled={sending}
        onClick={onSend}
        type="button"
      >
        <Send className="h-3.5 w-3.5" />
        {sending ? 'Sending…' : 'Send'}
      </button>

      <button
        aria-label="Schedule send"
        aria-pressed={showSchedulePicker}
        className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
          showSchedulePicker ? 'bg-accent/10 text-accent' : 'text-fg-muted hover:bg-bg-secondary'
        }`}
        disabled={sending}
        onClick={onToggleSchedulePicker}
        title="Schedule send"
        type="button"
      >
        <Clock className="h-4 w-4" />
      </button>
      {showSchedulePicker && (
        <input
          aria-label="Schedule send time"
          className="border-border bg-bg-secondary text-fg focus:border-accent h-8 w-44 max-w-full shrink-0 rounded-md border px-2 text-xs outline-none"
          min={localDatetimeMin()}
          onChange={(e) => onScheduleChange(e.target.value)}
          type="datetime-local"
          value={scheduledAt}
        />
      )}

      <div className="bg-border mx-0.5 h-4 w-px" />

      <button
        aria-label="AI polish"
        className="text-accent hover:bg-accent/10 disabled:text-fg-muted flex h-8 shrink-0 items-center rounded-md px-2 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        disabled={polishing || sending}
        onClick={onPolish}
        title="AI polish"
        type="button"
      >
        {polishing ? <Loader2 className="h-3 w-3 animate-spin" /> : 'Polish'}
      </button>

      {templates.length > 0 && (
        <select
          aria-label="Apply template"
          className="border-border bg-bg-secondary text-fg-secondary h-8 rounded-md border px-2 text-xs disabled:cursor-not-allowed disabled:opacity-50"
          defaultValue=""
          disabled={sending}
          onChange={(e) => {
            const t = templates.find((tt) => tt.id === Number(e.target.value))
            if (t) onTemplateSelect(t)
            e.target.value = ''
          }}
        >
          <option disabled value="">
            Templates
          </option>
          {templates.map((t) => (
            <option key={t.id} value={t.id}>
              {t.name}
            </option>
          ))}
        </select>
      )}

      <div className="flex-1" />
      <kbd
        aria-hidden="true"
        className="text-fg-muted mr-1 hidden text-[10px] select-none sm:inline"
      >
        {isMacLike() ? '⌘' : 'Ctrl+'}↵
      </kbd>
      <button
        className="text-fg-muted hover:bg-bg-secondary flex h-8 shrink-0 items-center rounded-md px-3 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50"
        disabled={sending}
        onClick={onCancel}
        type="button"
      >
        Cancel
      </button>
    </div>
  )
}

function isMacLike(): boolean {
  return typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.userAgent)
}

function localDatetimeMin(): string {
  const d = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}
