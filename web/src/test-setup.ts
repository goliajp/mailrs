// global test setup — mock browser APIs that gds modules require at import time

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
