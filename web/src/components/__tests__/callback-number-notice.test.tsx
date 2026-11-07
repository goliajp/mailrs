import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { CallbackNumberNotice } from '../callback-number-notice'
import { makeMessage } from './thread-view-fixtures'

/** `GET /api/senders/{address}/callback-numbers` as `CallbackNumbers` serializes (handlers/callback_numbers.rs). */
function history(...numbers: [string, string][]) {
  return {
    numbers: numbers.map(([number, first_message_id]) => ({
      first_message_id,
      first_seen: 1_790_000_000,
      number,
    })),
    sender: 'desk@hotel.example',
  }
}

function renderIt(text: string) {
  const message = makeMessage({
    message_id: 'b3@hotel.example',
    sender: 'Front Desk <Desk@Hotel.example>',
    text_body: text,
  })
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <CallbackNumberNotice conversation={[message]} message={message} />
    </QueryClientProvider>
  )
}

function stubFetch(body: unknown): ReturnType<typeof vi.fn> {
  const fn = vi.fn(
    async () =>
      new Response(JSON.stringify(body), {
        headers: { 'content-type': 'application/json' },
        status: 200,
      })
  )
  vi.stubGlobal('fetch', fn)
  return fn
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<CallbackNumberNotice />', () => {
  it('names only the numbers the sender never gave the reader in any conversation', async () => {
    const fetch = stubFetch(
      history(['0312345678', 'b1@hotel.example'], ['0570000999', 'b3@hotel.example'])
    )
    const { container } = renderIt('Call 03-1234-5678 or 0570-000-999 to change your booking.')
    await waitFor(() => expect(container.querySelector('[data-callback-notice]')).not.toBeNull())
    const notice = container.querySelector('[data-callback-notice]')
    expect(notice).toHaveTextContent('0570-000-999')
    expect(notice).not.toHaveTextContent('03-1234-5678')
    expect(String(fetch.mock.calls[0][0])).toContain(
      '/api/senders/desk%40hotel.example/callback-numbers'
    )
  })

  it('says nothing when every number was given before', async () => {
    const fetch = stubFetch(history(['0312345678', 'b1@hotel.example']))
    const { container } = renderIt('Call 03-1234-5678.')
    await waitFor(() => expect(fetch).toHaveBeenCalled())
    // the answer has been read once nothing is pending
    await new Promise((r) => setTimeout(r, 50))
    expect(container.querySelector('[data-callback-notice]')).toBeNull()
  })

  it('asks for no history when the message gives no number', () => {
    const fetch = stubFetch(history())
    const { container } = renderIt('See you on Friday.')
    expect(container.querySelector('[data-callback-notice]')).toBeNull()
    expect(fetch).not.toHaveBeenCalled()
  })
})
