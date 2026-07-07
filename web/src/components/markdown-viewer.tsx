// lazy split-out: react-markdown + remark-gfm + rehype-highlight together
// weigh ~150-200 kB minified once highlight.js's language pack is dragged in.
// most received emails are plain-text and never hit this code path, so
// loading it on demand keeps the chat hot path lean. consumers should wrap
// the import in React.lazy + <Suspense> and only render this when a
// `looksLikeMarkdown` heuristic actually flips to true.

import { useState } from 'react'
import Markdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'

function CodeBlock({ children, className, ...props }: React.HTMLAttributes<HTMLElement>) {
  const [copied, setCopied] = useState(false)
  const code = String(children).replace(/\n$/, '')
  const lang = className?.replace('language-', '') ?? ''

  const copy = () => {
    navigator.clipboard.writeText(code)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className="group relative overflow-hidden">
      {lang && (
        <span className="text-fg-muted md:text-mini absolute top-2 right-10 text-xs opacity-100 transition-opacity md:opacity-0 md:group-hover:opacity-100">
          {lang}
        </span>
      )}
      <button
        aria-label={copied ? 'Copied to clipboard' : 'Copy code'}
        className="touch-target text-fg-muted hover:bg-bg-secondary hover:text-fg md:text-mini absolute top-2 right-2 rounded-md px-1.5 py-0.5 text-xs opacity-100 transition-opacity md:opacity-0 md:group-hover:opacity-100"
        onClick={copy}
      >
        {copied ? 'Copied!' : 'Copy'}
      </button>
      <code className={className} {...props}>
        {children}
      </code>
    </div>
  )
}

const markdownComponents = {
  code: ({ children, className, ...props }: React.HTMLAttributes<HTMLElement>) => {
    const isBlock = className?.startsWith('language-') || String(children).includes('\n')
    if (isBlock) {
      return (
        <CodeBlock className={className} {...props}>
          {children}
        </CodeBlock>
      )
    }
    return (
      <code className={className} {...props}>
        {children}
      </code>
    )
  },
}

export function MarkdownViewer({ body }: { body: string }) {
  return (
    <Markdown
      components={markdownComponents}
      rehypePlugins={[rehypeHighlight]}
      remarkPlugins={[remarkGfm]}
    >
      {body}
    </Markdown>
  )
}

// default export so React.lazy can pick it up directly.
export default MarkdownViewer
