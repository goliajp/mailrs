import { Marked } from 'marked'

// The single markdown → HTML engine that BOTH the compose Preview tab
// and the send path use.
//
// Preview previously used react-markdown + remark-gfm + remark-breaks,
// which interpreted every `\n` as `<br>` so tightly that a numbered
// list on adjacent lines never parsed as a list at all — the Preview
// stripped `1.` / `2.` / `- ` markers while the recipient still saw
// a proper list (send went through marked). Two engines, two answers.
// Now one engine, one answer.
//
// Options:
//   - `async: false` — return synchronously; caller stores HTML in state
//   - `breaks: true` — newlines inside a paragraph become <br>, matching
//     what users typing plain paragraphs expect (marked is smart enough
//     to still parse lists, unlike remark-breaks which is not)
//   - `gfm: true`   — GitHub-flavoured markdown extensions
//
// What a person types is text, never markup: the HTML tokenizers are off,
// so `<b>` or `<script>` arrive escaped in `html_body` rather than as tags
// in the recipient's mail. A link becomes `<a>` only for http, https and
// mailto; anything else (`javascript:`, `data:`, relative paths) keeps its
// text and loses the link. The phone clients render the same subset the
// same way, held to it by `markdown-samples.json`.
const SAFE_HREF = /^(https?:|mailto:)/i

const engine = new Marked({
  async: false,
  breaks: true,
  gfm: true,
  renderer: {
    link(token) {
      if (SAFE_HREF.test(token.href)) return false
      return this.parser.parseInline(token.tokens)
    },
  },
  tokenizer: {
    html() {
      return undefined
    },
    tag() {
      return undefined
    },
  },
})

export function renderMarkdownToHtml(md: string): string {
  return engine.parse(md) as string
}

/** What a send puts in `html_body` for a markdown body: blank lines kept as space. */
export function renderMarkdownForSend(md: string): string {
  return renderMarkdownToHtml(preserveBlankLines(md))
}

// A run of blank lines keeps its height: one empty paragraph per extra line.
function preserveBlankLines(md: string): string {
  return md.replace(/\n{2,}/g, (match) => '\n\n' + '&nbsp;\n\n'.repeat(match.length - 1))
}
