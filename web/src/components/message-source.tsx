import { toast } from '@goliapkg/gds'
import { useState } from 'react'

import { wireGetMessageSource } from '@/wire/endpoints/mail'

/**
 * The message as it arrived, headers and all.
 *
 * The one view in this app that shows what the server was given rather
 * than what it made of it — which is where the answer lives when a
 * message landed in Junk, or claims to be from someone it is not.
 * `Authentication-Results` is the line worth finding, and it is near
 * the top.
 *
 * The route has been live since before this client existed and no web
 * page ever called it; iOS gained a viewer this week and this is the
 * other half.
 */
export function MessageSource({ uid }: { uid: number }) {
  const [source, setSource] = useState<null | string>(null)
  const [loading, setLoading] = useState(false)

  const load = async () => {
    setLoading(true)
    try {
      setSource(await wireGetMessageSource(uid))
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Could not read the source')
    } finally {
      setLoading(false)
    }
  }

  if (source !== null) {
    return (
      <div className="mt-2">
        <div className="mb-1 flex items-center gap-3">
          <button
            className="text-fg-muted hover:text-fg text-xs underline"
            onClick={() => setSource(null)}
          >
            Hide source
          </button>
          <button
            className="text-fg-muted hover:text-fg text-xs underline"
            onClick={() => {
              void navigator.clipboard.writeText(source)
              toast.success('Copied')
            }}
          >
            Copy
          </button>
        </div>
        {/* Monospaced and not re-wrapped: a folded header means
            something, and rewrapping it to the pane width would be
            showing a different message than the one that arrived. */}
        <pre className="bg-bg-secondary text-fg-muted max-h-96 overflow-auto rounded p-2 text-xs">
          {source}
        </pre>
      </div>
    )
  }

  return (
    <button
      className="text-fg-muted hover:text-fg mt-2 text-xs underline"
      disabled={loading}
      onClick={() => void load()}
    >
      {loading ? 'Loading source…' : 'View source'}
    </button>
  )
}
