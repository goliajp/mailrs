import { Check, Copy } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { copyText } from './copy-text'

type Variant = 'ghost' | 'solid'

const VARIANT_CLASS: Record<Variant, string> = {
  ghost:
    'text-fg-muted hover:text-fg hover:bg-bg-secondary border border-transparent hover:border-border',
  solid: 'bg-fg text-bg hover:opacity-90 border border-transparent',
}

/**
 * Copy-to-clipboard button that confirms in place for 1.4s. `label` names
 * what is being copied and is used for both the toast and the a11y name.
 */
export function CopyButton({
  className = '',
  label,
  value,
  variant = 'ghost',
  withText = false,
}: {
  className?: string
  label: string
  value: string
  variant?: Variant
  withText?: boolean
}) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<null | ReturnType<typeof setTimeout>>(null)

  useEffect(() => {
    return () => {
      if (timer.current) clearTimeout(timer.current)
    }
  }, [])

  const handleClick = () => {
    void copyText(value, label).then((ok) => {
      if (!ok) return
      setCopied(true)
      if (timer.current) clearTimeout(timer.current)
      timer.current = setTimeout(() => setCopied(false), 1400)
    })
  }

  const icon = renderIcon(copied)
  const classes = [
    'inline-flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1 text-xs transition-colors',
    'focus-visible:ring-accent/50 focus-visible:ring-2 focus-visible:outline-none',
    VARIANT_CLASS[variant],
    className,
  ].join(' ')

  return (
    <button aria-label={`Copy ${label}`} className={classes} onClick={handleClick} type="button">
      {icon}
      {withText && <span>{copyWord(copied)}</span>}
    </button>
  )
}

function copyWord(copied: boolean): string {
  if (copied) return 'Copied'
  return 'Copy'
}

function renderIcon(copied: boolean) {
  if (copied) return <Check aria-hidden className="text-success h-3.5 w-3.5" />
  return <Copy aria-hidden className="h-3.5 w-3.5" />
}
