import type { QuoteBlockData } from '../types'

import { ChevronDown, ChevronRight } from 'lucide-react'

import { HtmlFrame } from '@/components/html-frame'
import { type Key, useT } from '@/i18n'

type Props = {
  data: QuoteBlockData
  mode?: 'forward' | 'reply'
  onChange: (data: QuoteBlockData) => void
}

export function QuoteBlock({ data, mode, onChange }: Props) {
  const t = useT()
  const collapsed = data.collapsed
  // stitch the "On Date, Sender wrote:" header onto the body so the
  // preview matches what will actually be sent
  const previewHtml = data.headerHtml ? data.headerHtml + data.html : data.html

  return (
    <div className="border-border border-t border-l-2">
      <button
        className="text-fg-muted hover:bg-bg-secondary flex w-full cursor-pointer items-center gap-1 px-4 py-2 text-xs transition-colors"
        onClick={() => onChange({ ...data, collapsed: !collapsed })}
        type="button"
      >
        {collapsed ? <ChevronRight className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
        {t(quoteToggle(collapsed, mode === 'forward'))}
      </button>
      {!collapsed && previewHtml && (
        <div className="border-border border-l-2">
          {/* iframe preserves the sender's styling (tables, inline css,
              images) that a Tiptap-based preview would have flattened.
              capped height keeps long newsletters from pushing the editor
              off-screen — user scrolls inside the preview box */}
          <HtmlFrame html={previewHtml} maxHeight="50vh" />
        </div>
      )}
    </div>
  )
}

function quoteToggle(collapsed: boolean, forwarded: boolean): Key {
  if (!collapsed) return 'composer.hideOriginal'
  if (forwarded) return 'composer.showOriginalForwarded'
  return 'composer.showOriginal'
}
