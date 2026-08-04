import type { ConversationSummary, ThreadMessage } from '@/lib/types'

/**
 * Row and message builders for the ThreadView suite.
 *
 * Their own module because `vi.mock` hoists per file and the spec does
 * not — so a fixture that grows a field should not push the file it is
 * declared in over the size limit, and these are the part of that file
 * with nothing test-specific in them.
 */

export function makeConversation(
  overrides: Partial<ConversationSummary> = {}
): ConversationSummary {
  return {
    archived: false,
    category: 'general',
    flagged: false,
    importance_level: 'normal',
    importance_score: 0.3,
    last_date: Math.floor(Date.now() / 1000),
    message_count: 1,
    participants: ['alice@example.com'],
    pinned: false,
    received_count: 1,
    requires_action: false,
    sent_count: 0,
    snippet: 'A snippet',
    subject: 'Test Subject',
    thread_id: 'thread-1',
    unread_count: 0,
    ...overrides,
  }
}

export function makeMessage(overrides: Partial<ThreadMessage> = {}): ThreadMessage {
  return {
    action_deadline: null,
    action_items: [],
    ai_analyzed: false,
    amounts: [],
    attachments: [],
    category: 'general',
    clean_text: null,
    dates: [],
    flags: 0,
    has_tracking_pixel: false,
    html_body: null,
    importance_level: 'normal',
    importance_score: 0.3,
    internal_date: 1700000000,
    is_bulk_sender: false,
    message_id: '<msg1@example.com>',
    new_content: null,
    people: [],
    recipients: 'bob@example.com',
    requires_action: false,
    risk_reason: '',
    risk_score: 0,
    sender: 'Alice Smith <alice@example.com>',
    sender_intent: 'inform',
    sender_trust: 'verified',
    subject: 'Test Subject',
    summary: '',
    text_body: 'Hello, this is a test message',
    uid: 100,
    ...overrides,
  }
}
