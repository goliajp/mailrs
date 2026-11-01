import { describe, expect, it } from 'vitest'

import { entryScript, isStaleBuild } from '@/lib/build-entry'

// the deployed index.html, as `vite build` writes it
const DEPLOYED = `<!doctype html>
<html lang="en">
  <head>
    <title>Mailrs</title>
    <script type="module" crossorigin src="/assets/index-BpspHz2f.js"></script>
  </head>
  <body><div id="root"></div></body>
</html>`

const DEV = `<script type="module" src="/src/main.tsx"></script>`

describe('which build a page runs', () => {
  it('reads the hashed entry script out of a built index.html', () => {
    expect(entryScript(DEPLOYED)).toBe('/assets/index-BpspHz2f.js')
  })

  it('finds none under the dev server', () => {
    expect(entryScript(DEV)).toBeNull()
  })

  it('is stale when the deployed page names another entry', () => {
    expect(isStaleBuild('/assets/index-OLDOLDOL.js', DEPLOYED)).toBe(true)
  })

  it('is current when it names the same one', () => {
    expect(isStaleBuild('/assets/index-BpspHz2f.js', DEPLOYED)).toBe(false)
  })

  it('never calls a page stale on an answer it cannot read', () => {
    // a login wall, an error page, the dev server
    expect(isStaleBuild('/assets/index-BpspHz2f.js', '<html>502</html>')).toBe(false)
    expect(isStaleBuild(null, DEPLOYED)).toBe(false)
  })
})
