import type { CreatedAgentKey } from '../_shared'

import { KeyRound, X } from 'lucide-react'
import { useRef } from 'react'

import { CopyButton } from './copy-button'

/**
 * One-shot secret reveal. The server keeps only the 8-char prefix
 * (`complete.rs:1340`), so once this panel is dismissed the full key is
 * unrecoverable — the copy is deliberately loud about that.
 */
export function CreatedKeyPanel({
  created,
  onDismiss,
}: {
  created: CreatedAgentKey
  onDismiss: () => void
}) {
  const input = useRef<HTMLInputElement>(null)

  return (
    <section
      aria-label="New API key"
      className="border-warning/60 bg-warning/5 shadow-elevation-1 rounded-lg border"
      role="status"
    >
      <header className="border-warning/25 flex items-center gap-2 border-b px-4 py-2.5">
        <KeyRound aria-hidden className="text-warning h-4 w-4" />
        <h3 className="text-sm font-semibold tracking-tight">Secret key — shown once</h3>
        <button
          aria-label="Dismiss"
          className="text-fg-muted hover:bg-bg-secondary hover:text-fg ml-auto rounded-md p-1 transition-colors"
          onClick={onDismiss}
          type="button"
        >
          <X aria-hidden className="h-4 w-4" />
        </button>
      </header>

      <div className="space-y-3 px-4 py-3.5">
        <div className="flex items-stretch gap-2">
          <input
            aria-label="Secret key"
            className="border-border bg-bg-secondary text-fg focus:border-accent focus:ring-accent/30 text-mid min-w-0 flex-1 rounded-md border px-3 py-2 font-mono tracking-tight select-all focus:ring-1 focus:outline-none"
            onFocus={() => input.current?.select()}
            readOnly
            ref={input}
            value={created.key}
          />
          <CopyButton
            className="px-3"
            label="API key"
            value={created.key}
            variant="solid"
            withText
          />
        </div>
        <p className="text-fg-secondary text-xs">
          Store it in your secret manager now. mailrs keeps only the{' '}
          <code className="bg-bg-secondary text-mini rounded px-1 py-0.5 font-mono">
            {created.prefix}
          </code>{' '}
          prefix, so this value cannot be shown again — a lost key has to be revoked and replaced.
        </p>
      </div>
    </section>
  )
}
