import type { AnyBlock } from './types'

import { useMemo } from 'react'

import { previewHtml } from './preview'

/**
 * The whole mail, as the person receiving it will get it.
 *
 * Rendered in a sandboxed `srcDoc` iframe rather than in the page: the
 * document carries its own `<body>` styles and a 600px wrapper, and
 * nothing of this app's stylesheet reaches a recipient. Dropping the
 * same markup into a `<div>` here would let the app's font, colours and
 * dark background decide how it looks — which is the mistake the old
 * per-block Preview made, and why what was approved never matched what
 * arrived.
 *
 * `sandbox` with no allowances: no scripts, no forms, no navigation.
 * The content is the sender's own, but it goes through the same door as
 * anything else that gets rendered, and a preview has no reason to be
 * able to run anything.
 */
export function EmailPreview({ blocks }: { blocks: ReadonlyArray<AnyBlock> }) {
  const html = useMemo(() => previewHtml(blocks), [blocks])

  if (!html) {
    return (
      <div className="flex min-h-[240px] items-center justify-center px-4">
        <p className="text-fg-muted text-sm">Nothing to preview yet.</p>
      </div>
    )
  }

  return (
    <iframe
      className="min-h-[240px] w-full flex-1 border-0 bg-white"
      sandbox=""
      srcDoc={html}
      title="Preview of the message as the recipient will see it"
    />
  )
}
