import type { ReactNode } from 'react'

type Props = {
  className?: string
  /** What one line of the body becomes: links, mentions. */
  renderLine?: (line: string) => ReactNode[]
  text: string
}

// one block per line of the body, with the space between lines as a
// margin rather than line-height: WebKit paints a selected line from the
// bottom of the line above, so a tall line-height leaves the highlight
// sitting low under the text. a long line that wraps keeps the font's own
// line-height.
export function PlainTextLines({ className, renderLine, text }: Props) {
  return (
    <div className={className}>
      {text.split('\n').map((line, i) => (
        // index key: a static split of one string
        <div className="mb-[3px] whitespace-pre-wrap select-text last:mb-0" key={i}>
          {line === '' && <br />}
          {line !== '' && (renderLine?.(line) ?? line)}
        </div>
      ))}
    </div>
  )
}
