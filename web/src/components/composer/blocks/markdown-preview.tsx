import Markdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkBreaks from 'remark-breaks'
import remarkGfm from 'remark-gfm'

// lazy-loaded markdown preview body for the compose `Preview` tab. extracted
// so the eager compose path doesn't pull react-markdown + rehype-highlight
// (≈150-200 kB once highlight.js languages are in the graph).

export function MarkdownPreview({ content }: { content: string }) {
  return (
    <Markdown rehypePlugins={[rehypeHighlight]} remarkPlugins={[remarkGfm, remarkBreaks]}>
      {content}
    </Markdown>
  )
}

export default MarkdownPreview
