/**
 * Which build a page is running, told by the entry script its
 * `index.html` names.
 *
 * Every build gives the entry a new hashed name (`/assets/index-<hash>.js`),
 * so two different names mean two different builds. The version string
 * cannot say this: a direct deploy calls itself `direct-<version>`, a web
 * tag a date, and neither is the backend's version.
 */

const ENTRY = /<script\b[^>]*\btype="module"[^>]*\bsrc="([^"]*\/assets\/[^"]+\.js)"/

/** The entry script path in an `index.html`, or null when it has none (the dev server). */
export function entryScript(html: string): null | string {
  return ENTRY.exec(html)?.[1] ?? null
}

/** Whether the deployed `index.html` names a different build from the loaded one. */
export function isStaleBuild(loaded: null | string, deployedHtml: string): boolean {
  const deployed = entryScript(deployedHtml)
  return loaded !== null && deployed !== null && deployed !== loaded
}

/** The entry script path this page was loaded with, or null under the dev server. */
export function loadedEntryScript(doc: Document): null | string {
  const el = doc.querySelector<HTMLScriptElement>('script[type="module"][src*="/assets/"]')
  if (!el) return null
  return new URL(el.src, doc.baseURI).pathname
}
