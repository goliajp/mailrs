import { Check, Copy } from 'lucide-react'
import { useCallback, useState } from 'react'

import { type Translate, useT } from '@/i18n'

// wrapper that shows copy button on hover
export function Copyable({
  children,
  className,
  value,
}: {
  children: React.ReactNode
  className?: string
  value: string
}) {
  return (
    <span className={`group/copy inline-flex items-center gap-0.5 ${className ?? ''}`}>
      <span className="select-text">{children}</span>
      <CopyButton value={value} />
    </span>
  )
}

// inline copy button shown on hover next to copyable identifiers
export function CopyButton({ className, value }: { className?: string; value: string }) {
  const t = useT()
  const [copied, setCopied] = useState(false)

  const handleCopy = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      navigator.clipboard.writeText(value).then(() => {
        setCopied(true)
        setTimeout(() => setCopied(false), 1500)
      })
    },
    [value]
  )

  return (
    <button
      aria-label={copyLabel(copied, value, t)}
      className={`touch-target text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary inline-flex shrink-0 items-center justify-center rounded-md p-1 opacity-100 transition-opacity md:p-0.5 md:opacity-0 md:group-hover/copy:opacity-100 ${className ?? ''}`}
      onClick={handleCopy}
      title={copyTitle(copied, value, t)}
    >
      {copied ? <Check className="text-success h-3 w-3" /> : <Copy className="h-3 w-3" />}
    </button>
  )
}

function copyLabel(copied: boolean, value: string, t: Translate): string {
  if (copied) return t('copy.copied')
  return t('copy.label', { value })
}

function copyTitle(copied: boolean, value: string, t: Translate): string {
  if (copied) return t('copy.copiedBang')
  return t('copy.title', { value })
}
