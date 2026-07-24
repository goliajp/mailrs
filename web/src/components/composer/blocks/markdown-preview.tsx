import DOMPurify from 'dompurify'
import { useMemo } from 'react'

import { renderMarkdownToHtml } from '@/lib/render-markdown'

// Lazy-loaded markdown preview body for the compose `Preview` tab.
//
// Renders through the exact same marked call the send path uses
// (`@/lib/render-markdown`) so the recipient can never see something
// different from what the sender saw in Preview. The previous
// react-markdown + remark-breaks pipeline diverged from marked on
// tight lists — numbered / bulleted items on adjacent lines lost
// their markers in preview but kept them when sent, so what the
// sender pre-approved in Preview was not what went out.
//
// DOMPurify sanitises marked's output before we inject it. marked
// already escapes user text, but adding sanitisation is the
// standard belt-and-braces for dangerouslySetInnerHTML: if a future
// marked plugin or version widens what gets emitted, this catches
// XSS before the preview renders it.

export function MarkdownPreview({ content }: { content: string }) {
  const html = useMemo(() => DOMPurify.sanitize(renderMarkdownToHtml(content)), [content])
  return <div dangerouslySetInnerHTML={{ __html: html }} />
}

export default MarkdownPreview
