import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { createRef } from 'react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { makeMessage } from '@/components/__tests__/thread-view-fixtures'

import { ThreadStack } from '../thread-stack'

vi.mock('@/components/thread-reply-box', () => ({
  ThreadReplyBox: () => <div data-testid="reply-box" />,
}))
vi.mock('@/components/message-bubble', () => ({ MessageBubble: () => <div /> }))
vi.mock('@/components/ai-analysis', () => ({ AiAnalysisPanel: () => null }))
vi.mock('@/components/attachment-preview', () => ({ AttachmentPreview: () => null }))
vi.mock('@/components/fraud-verdict-panel', () => ({ FraudVerdictPanel: () => null }))

afterEach(cleanup)

const noop = () => {}
const CONTROLS = {
  goToNext: noop,
  goToPrev: noop,
  hasNext: false,
  hasPrev: false,
  isArchived: false,
  isFlagged: false,
  isRead: true,
  isTrashed: false,
  onArchive: noop,
  onMarkRead: noop,
  onMarkUnread: noop,
  onStar: noop,
  onTrash: noop,
  onUnstar: noop,
  onUntrash: noop,
}

function headers(stack: ReturnType<typeof show>) {
  return stack
    .getAllByRole('button', { expanded: undefined })
    .filter((b) => b.hasAttribute('data-stack-header'))
}

/** Seven read messages from different senders; the fourth unread. */
function longConversation() {
  return Array.from({ length: 7 }, (_, i) =>
    makeMessage({
      flags: i === 3 ? 0 : 1,
      internal_date: 1_700_000_000 + i * 3600,
      sender: `Person ${i + 1} <p${i + 1}@example.com>`,
      text_body: `body of message ${i + 1}`,
      uid: i + 1,
    })
  )
}

function show() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Provider store={createStore()}>
        <MemoryRouter>
          <ThreadStack
            controls={CONTROLS}
            focusedUid={null}
            loadingThread={false}
            messages={longConversation()}
            myEmail="me@example.com"
            onForward={noop}
            onReply={noop}
            onReplyModeChange={noop}
            onReplySent={noop}
            replyCtx={{
              forwardAttachmentsUid: null,
              forwardMessageId: null,
              lastMessageId: 'm7',
              originalBody: '',
              originalDate: '',
              originalFrom: '',
              originalHtmlBody: null,
              replyAllRecipients: '',
              replyRecipients: '',
              subject: 'Test Subject',
              threadId: 't1',
            }}
            replyMode="reply"
            scrollRef={createRef()}
            subject="Test Subject"
            threadId="t1"
          />
        </MemoryRouter>
      </Provider>
    </QueryClientProvider>
  )
  return within(screen.getByRole('group', { name: 'Messages in this conversation' }))
}

describe('a long conversation as a stack', () => {
  it('opens the newest and the unread message, and folds the older run', () => {
    const stack = show()
    const open = headers(stack).filter((h) => h.getAttribute('aria-expanded') === 'true')
    expect(open.map((h) => h.textContent)).toEqual([
      expect.stringContaining('Person 4'),
      expect.stringContaining('Person 7'),
    ])
    // 2 and 3 are one line; 5, 6 are among the newest three and stay
    expect(stack.getByRole('button', { name: 'Show 2 earlier messages' })).toBeDefined()
    expect(stack.queryByText(/Person 2/)).toBeNull()
    expect(screen.getByTestId('reply-box')).toBeDefined()
  })

  it('opens a folded message when its row is clicked, and folds it again', () => {
    const stack = show()
    const row = headers(stack).find((h) => h.textContent?.includes('Person 5'))
    expect(row?.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(row!)
    const opened = headers(stack).find((h) => h.textContent?.includes('Person 5'))
    expect(opened?.getAttribute('aria-expanded')).toBe('true')
    expect(stack.getByText('body of message 5')).toBeDefined()
    fireEvent.click(opened!)
    // folded again: one line, its text shortened into the row
    const folded = headers(stack).find((h) => h.textContent?.includes('Person 5'))
    expect(folded?.getAttribute('aria-expanded')).toBe('false')
  })

  it('shows the folded run when asked', () => {
    const stack = show()
    fireEvent.click(stack.getByRole('button', { name: 'Show 2 earlier messages' }))
    expect(stack.getByText(/Person 2/)).toBeDefined()
    expect(stack.getByText(/Person 3/)).toBeDefined()
  })

  it('moves between messages with the keys, and keeps them from the list', () => {
    const listKeys = vi.fn()
    window.addEventListener('keydown', listKeys)
    const stack = show()
    const first = headers(stack)[0]
    first.focus()
    fireEvent.keyDown(first, { key: 'ArrowDown' })
    // the folded run is a line, not a header: the next header is message 4
    expect(document.activeElement?.textContent).toContain('Person 4')
    fireEvent.keyDown(document.activeElement!, { key: 'j' })
    expect(document.activeElement?.textContent).toContain('Person 5')
    fireEvent.keyDown(document.activeElement!, { key: 'k' })
    expect(document.activeElement?.textContent).toContain('Person 4')
    fireEvent.keyDown(document.activeElement!, { key: 'End' })
    expect(document.activeElement?.textContent).toContain('Person 7')
    // o opens or folds the one with focus
    fireEvent.keyDown(document.activeElement!, { key: 'Home' })
    fireEvent.keyDown(document.activeElement!, { key: 'o' })
    expect(stack.getByText('body of message 1')).toBeDefined()
    expect(listKeys).not.toHaveBeenCalled()
    window.removeEventListener('keydown', listKeys)
  })
})
