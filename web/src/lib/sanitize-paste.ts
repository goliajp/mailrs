/**
 * Cleanup pasted HTML before it lands in the rich-text editor.
 *
 * Background (MRS-17): a Weekly Report email composed via the reply-box
 * went out with the signature wrapped into a bullet list. Root cause was
 * that an external clipboard source (Slack / Notion / iMessage / a Markdown
 * preview) emits HTML where:
 *
 *   - <li> tags carry a leading literal "- " on top of the bullet the
 *     <li> already provides (so recipient sees "• - Item"),
 *   - empty <li>s and <li> with only signature text get glued to the
 *     same <ul> as the real list,
 *   - inline `style` and `class` attributes from the source app fight
 *     with the editor's own theme.
 *
 * TipTap's StarterKit accepts the HTML as-is. The recipient's HTML email
 * client then renders the broken nesting verbatim.
 *
 * This module sanitizes the clipboard HTML *before* TipTap parses it.
 * Conservative: we don't move blocks around (that would surprise users),
 * we only remove the mechanical noise that no human meant to paste.
 */
export function sanitizePastedHtml(html: string): string {
  // Wrap so we always have a single root to walk.
  const doc = new DOMParser().parseFromString(`<div>${html}</div>`, 'text/html')
  const root = doc.body.firstElementChild
  if (!root) return html

  // 1. Strip leading bullet markers from <li> text content. Many
  //    Markdown-rendered sources duplicate the marker as literal text
  //    on top of the structural <li>.
  const LEADING_MARKER = /^[\s\u00a0]*[-*+•·]\s+/
  for (const li of Array.from(root.querySelectorAll('li'))) {
    const first = li.firstChild
    if (first && first.nodeType === Node.TEXT_NODE) {
      const t = first.textContent ?? ''
      first.textContent = t.replace(LEADING_MARKER, '')
    }
  }

  // 2. Remove empty <li>s left by the producer (e.g. blank lines that
  //    shouldn't have been list items).
  for (const li of Array.from(root.querySelectorAll('li'))) {
    if (!li.textContent?.trim() && li.children.length === 0) {
      li.remove()
    }
  }

  // 3. Remove now-empty <ul>/<ol>.
  for (const list of Array.from(root.querySelectorAll('ul, ol'))) {
    if (!list.children.length) list.remove()
  }

  // 4. Strip inline style and class noise so the editor's theme governs
  //    presentation. This also rids us of MS-Word `mso-*` cruft and
  //    Slack / Notion color overrides.
  for (const el of Array.from(root.querySelectorAll<HTMLElement>('[style], [class]'))) {
    el.removeAttribute('style')
    el.removeAttribute('class')
  }

  // 5. Unwrap decorative `<font>` and bare `<span>` elements (no
  //    semantic value — typically purely styling).
  for (const el of Array.from(root.querySelectorAll('font, span'))) {
    while (el.firstChild) el.parentNode?.insertBefore(el.firstChild, el)
    el.remove()
  }

  return root.innerHTML
}
