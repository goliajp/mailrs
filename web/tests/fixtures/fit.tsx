import { createRoot } from 'react-dom/client'

import { HtmlFrame } from '@/components/html-frame'

const width = Number(new URLSearchParams(location.search).get('w') ?? '600')
const html = `<table width="${width}" style="width:${width}px"><tr><td>
  <div style="width:${width}px;height:400px;background:#eee">wide ${width}</div>
</td></tr></table>`

createRoot(document.getElementById('col')!).render(<HtmlFrame html={html} />)
