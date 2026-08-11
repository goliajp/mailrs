import type { AttachmentInfo } from '@/lib/types'

import { describe, expect, it } from 'vitest'

import { referencedCids, visibleAttachments } from '@/lib/inline-attachments'

function att(over: Partial<AttachmentInfo> = {}): AttachmentInfo {
  return {
    content_id: null,
    content_type: 'application/pdf',
    filename: 'invoice.pdf',
    size: 1024,
    ...over,
  }
}

/** The signature graphic from the Outlook forward that prompted this. */
const folderIcon = att({
  content_id: '<image001.png@01DD28D9.8ED98FE0>',
  content_type: 'image/png',
  filename: 'image001.png',
  size: 207,
})

const bodyUsingIt = '<p>hi</p><img src="cid:image001.png@01DD28D9.8ED98FE0">'

describe('referencedCids', () => {
  it('finds nothing in html that references nothing', () => {
    expect(referencedCids('<p>plain</p>').size).toBe(0)
  })

  it('reads a cid regardless of quoting or case', () => {
    const html = `<img src="cid:A@b.com"><img src='cid:C@d.com'><img src=cid:E@f.com >`
    expect([...referencedCids(html)].sort()).toEqual(['a@b.com', 'c@d.com', 'e@f.com'])
  })
})

describe('visibleAttachments', () => {
  it('hides an inline image the body already draws', () => {
    expect(visibleAttachments([folderIcon], bodyUsingIt)).toEqual([])
  })

  it('keeps an inline part nobody references — otherwise it is unreachable', () => {
    const kept = visibleAttachments([folderIcon], '<p>no images here</p>')
    expect(kept).toHaveLength(1)
  })

  it('keeps a real attachment sitting beside a decoration', () => {
    const kept = visibleAttachments([folderIcon, att()], bodyUsingIt)
    expect(kept.map((k) => k.att.filename)).toEqual(['invoice.pdf'])
  })

  /**
   * The download URL is built from this index. Renumbering after a
   * filter would serve attachment 0 when the reader asked for 1 — a
   * wrong file, silently, which is worse than the noise being removed.
   */
  it('reports the original index, not the position after filtering', () => {
    const kept = visibleAttachments([folderIcon, att({ filename: 'a.pdf' })], bodyUsingIt)
    expect(kept[0].index).toBe(1)
  })

  it('leaves everything alone when the body has no cid at all', () => {
    const all = [att({ filename: 'a.pdf' }), att({ filename: 'b.pdf' })]
    expect(visibleAttachments(all, null).map((k) => k.index)).toEqual([0, 1])
  })
})
