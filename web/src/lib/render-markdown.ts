import { marked } from 'marked'

// The single markdown → HTML call that BOTH the compose Preview tab
// and the send path use.
//
// Preview previously used react-markdown + remark-gfm + remark-breaks,
// which interpreted every `\n` as `<br>` so tightly that a numbered
// list on adjacent lines never parsed as a list at all — the Preview
// stripped `1.` / `2.` / `- ` markers while the recipient still saw
// a proper list (send went through marked). Two engines, two answers.
// Now one call, one answer.
//
// Options match structured-compose's send-time invocation exactly:
//   - `async: false` — return synchronously; caller stores HTML in state
//   - `breaks: true` — newlines inside a paragraph become <br>, matching
//     what users typing plain paragraphs expect (marked is smart enough
//     to still parse lists, unlike remark-breaks which is not)
//   - `gfm: true`   — GitHub-flavoured markdown extensions
export function renderMarkdownToHtml(md: string): string {
  return marked.parse(md, { async: false, breaks: true, gfm: true }) as string
}
