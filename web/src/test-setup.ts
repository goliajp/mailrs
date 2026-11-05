import '@testing-library/jest-dom/vitest'

import { cleanup, configure } from '@testing-library/react'
import { afterEach } from 'vitest'

// `findBy*` and `waitFor` give up after one second of wall clock by
// default. Most pages under test wait on nothing but CPU: the fetch is
// stubbed and resolves at once, so an admin table that shows in 3-20 ms
// on an idle machine took over a second in the full parallel suite on a
// loaded one, and the first test in a file, which pays for first-time
// code paths, failed while the rest passed. Same reasoning as
// `testTimeout` in vite.config.ts: the budget is not a speed check. A
// test that waits on a real timer (a retry delay, a debounce) still says
// so at the call.
configure({ asyncUtilTimeout: 5_000 })

// Vitest doesn't auto-cleanup React test roots between tests; without
// this you get "found multiple elements by data-testid" the moment two
// tests mount the same component.
afterEach(() => {
  cleanup()
})

// global test setup — mock browser APIs that gds modules require at import time

// bun runtime omits localStorage from jsdom (it normally needs --localstorage-file).
// Provide a Map-backed shim so module-load reads don't throw; tests that need
// custom behavior still override via vi.stubGlobal('localStorage', ...).
function makeStorage(): Storage {
  const store = new Map<string, string>()
  return {
    clear: () => store.clear(),
    getItem: (k) => store.get(k) ?? null,
    key: (i) => [...store.keys()][i] ?? null,
    get length() {
      return store.size
    },
    removeItem: (k) => {
      store.delete(k)
    },
    setItem: (k, v) => {
      store.set(k, v)
    },
  }
}
globalThis.localStorage = makeStorage()

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => {},
    addListener: () => {},
    dispatchEvent: () => false,
    removeEventListener: () => {},
    removeListener: () => {},
  }),
})
