import type { ReactNode } from 'react'

import { splitUrls } from '@/lib/linkify'

// expand the string parts of an already-processed node list (e.g. the
// output of highlightMentions) into text + clickable anchors for bare
// http(s) URLs. non-string nodes (mention spans, etc.) pass through
// untouched. keeps URL detection in one place for both the reading pane
// and the chat bubbles.
export function linkifyNodes(nodes: ReactNode[], anchorClass: string): ReactNode[] {
  const out: ReactNode[] = []
  nodes.forEach((node, ni) => {
    if (typeof node !== 'string') {
      out.push(node)
      return
    }
    // index key OK: segments are a static, position-stable expansion of
    // one string — never reordered or inserted mid-list.
    splitUrls(node).forEach((seg, si) => {
      if (seg.type === 'url') {
        out.push(
          <a
            className={anchorClass}
            href={seg.value}
            key={`${ni}-${si}`}
            rel="noopener noreferrer"
            target="_blank"
          >
            {seg.value}
          </a>
        )
      } else {
        out.push(seg.value)
      }
    })
  })
  return out
}
