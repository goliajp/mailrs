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
