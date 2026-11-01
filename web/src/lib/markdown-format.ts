/**
 * Markdown formatting applied to a textarea's value and selection, for the
 * composer's toolbar and its Cmd/Ctrl+B/I/K keys. Pure: the text block
 * writes the result back through its own onChange.
 */

export type Edit = { end: number; start: number; value: string }

export type FormatKind = 'bold' | 'code' | 'italic' | 'link' | 'list' | 'numbered' | 'quote'

const WRAPS: Record<'bold' | 'code' | 'italic', string> = { bold: '**', code: '`', italic: '_' }

export function applyFormat(value: string, start: number, end: number, kind: FormatKind): Edit {
  switch (kind) {
    case 'bold':
    case 'code':
    case 'italic':
      return wrap(value, start, end, WRAPS[kind])
    case 'link':
      return link(value, start, end)
    case 'list':
      return prefixLines(value, start, end, () => '- ')
    case 'numbered':
      return prefixLines(value, start, end, (i) => `${i + 1}. `)
    case 'quote':
      return prefixLines(value, start, end, () => '> ')
  }
}

/** The kind a Cmd/Ctrl key combination asks for, or null. */
export function formatForKey(key: string): FormatKind | null {
  switch (key.toLowerCase()) {
    case 'b':
      return 'bold'
    case 'i':
      return 'italic'
    case 'k':
      return 'link'
    default:
      return null
  }
}

function lineEnd(value: string, at: number): number {
  const next = value.indexOf('\n', at)
  if (next === -1) return value.length
  return next
}

// the link text stays as typed and the url placeholder is selected, ready to paste over
function link(value: string, start: number, end: number): Edit {
  const picked = value.slice(start, end)
  const url = 'https://'
  const head = `${value.slice(0, start)}[${picked}](`
  return {
    end: head.length + url.length,
    start: head.length,
    value: `${head}${url})${value.slice(end)}`,
  }
}

// every line the selection touches, from the start of its first line
function prefixLines(
  value: string,
  start: number,
  end: number,
  prefix: (index: number) => string
): Edit {
  const from = value.lastIndexOf('\n', start - 1) + 1
  const to = lineEnd(value, end)
  const lines = value.slice(from, to).split('\n')
  const body = lines.map((line, i) => prefix(i) + line).join('\n')
  return {
    end: from + body.length,
    start: from,
    value: value.slice(0, from) + body + value.slice(to),
  }
}

// wrapping an already wrapped selection takes the marks off again
function wrap(value: string, start: number, end: number, mark: string): Edit {
  const before = value.slice(0, start)
  const picked = value.slice(start, end)
  const after = value.slice(end)
  if (before.endsWith(mark) && after.startsWith(mark)) {
    return {
      end: end - mark.length,
      start: start - mark.length,
      value: before.slice(0, -mark.length) + picked + after.slice(mark.length),
    }
  }
  return {
    end: end + mark.length,
    start: start + mark.length,
    value: before + mark + picked + mark + after,
  }
}
