// The phone reads a thread under the same domain scope as the desktop
// pane. It passed no domains, so a reader narrowed to one domain was
// shown messages of a thread from outside it.

import { cleanup, render } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

const asked: Array<[null | string, readonly string[]]> = []
vi.mock('@/hooks/use-mail-queries', () => ({
  useThreadQuery: (id: null | string, domains: readonly string[]) => {
    asked.push([id, domains])
    return { data: [], isPending: false }
  },
}))
vi.mock('@/hooks/use-current-list', () => ({
  useConversationRows: () => ({ rows: [] }),
  useCurrentThreadMessages: () => [],
  useSelectedThreadId: () => 't-1',
}))
vi.mock('@/hooks/use-mark-read-on-open', () => ({ useMarkReadOnOpen: () => undefined }))

import { MobileMail } from '@/components/mobile-mail'
import { mobileViewAtom, selectedDomainsAtom } from '@/store/ui'

// jsdom has no element scrolling
Element.prototype.scrollTo = () => {}

afterEach(cleanup)

describe('mobile thread view', () => {
  it('reads the thread under the selected domains', () => {
    const store = createStore()
    store.set(mobileViewAtom, 'thread')
    store.set(selectedDomainsAtom, ['golia.jp'])
    render(
      <Provider store={store}>
        <MobileMail />
      </Provider>
    )
    expect(asked.at(-1)).toEqual(['t-1', ['golia.jp']])
  })
})
