import { Check, Copy } from 'lucide-react'
import { useCallback, useState } from 'react'

// inline copy button shown on hover next to copyable identifiers
export function CopyButton({ value, className }: { value: string; className?: string }) {
  const [copied, setCopied] = useState(false)

  const handleCopy = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      navigator.clipboard.writeText(value).then(() => {
        setCopied(true)
        setTimeout(() => setCopied(false), 1500)
      })
    },
    [value],
  )

  return (
    <button
      onClick={handleCopy}
      title={copied ? 'Copied!' : `Copy "${value}"`}
      aria-label={copied ? 'Copied' : `Copy ${value}`}
      className={`inline-flex shrink-0 items-center justify-center rounded-md p-0.5 text-[var(--color-text-tertiary)] opacity-0 transition-opacity hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)] group-hover/copy:opacity-100 ${className ?? ''}`}
    >
      {copied ? (
        <Check className="h-3 w-3 text-[var(--color-status-success)]" />
      ) : (
        <Copy className="h-3 w-3" />
      )}
    </button>
  )
}

// wrapper that shows copy button on hover
export function Copyable({
  value,
  children,
  className,
}: {
  value: string
  children: React.ReactNode
  className?: string
}) {
  return (
    <span className={`group/copy inline-flex items-center gap-0.5 ${className ?? ''}`}>
      <span className="select-text">{children}</span>
      <CopyButton value={value} />
    </span>
  )
}
