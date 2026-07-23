import type { ThreadMessage } from '@/lib/types'

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { AiAnalysisPanel } from '@/components/ai-analysis'

afterEach(() => {
  cleanup()
})

function makeMessage(overrides: Partial<ThreadMessage> = {}): ThreadMessage {
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
    sender: 'alice@example.com',
    sender_intent: 'inform',
    sender_trust: 'verified',
    subject: 'Test',
    summary: '',
    text_body: 'text',
    uid: 100,
    ...overrides,
  }
}

describe('AiAnalysisPanel', () => {
  it('returns null when message is not ai_analyzed', () => {
    const { container } = render(<AiAnalysisPanel message={makeMessage({ ai_analyzed: false })} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders summary when present', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          ai_analyzed: true,
          summary: 'This email is about a meeting.',
        })}
      />
    )
    expect(screen.getByText('This email is about a meeting.')).toBeDefined()
  })

  it('renders risk reason when risk_score > 0', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          ai_analyzed: true,
          risk_reason: 'Contains suspicious links',
          risk_score: 50,
        })}
      />
    )
    expect(screen.getByText(/Contains suspicious links/)).toBeDefined()
  })

  it('does not render risk reason when risk_score is 0', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          ai_analyzed: true,
          risk_reason: 'Should not show',
          risk_score: 0,
        })}
      />
    )
    expect(screen.queryByText(/Should not show/)).toBeNull()
  })

  it('renders people mentions', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          ai_analyzed: true,
          people: [{ name: 'John Doe', role: 'Manager' }, { name: 'Jane Smith' }],
        })}
      />
    )
    // expand details
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('People')).toBeDefined()
    expect(screen.getByText('John Doe')).toBeDefined()
    expect(screen.getByText('(Manager)')).toBeDefined()
    expect(screen.getByText('Jane Smith')).toBeDefined()
  })

  it('renders date mentions', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          ai_analyzed: true,
          dates: [{ context: 'deadline', iso_date: '2025-03-15', text: 'March 15' }],
        })}
      />
    )
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('Dates')).toBeDefined()
    expect(screen.getByText('March 15')).toBeDefined()
  })

  it('renders amount mentions', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          ai_analyzed: true,
          amounts: [
            {
              context: 'invoice total',
              currency: 'USD',
              text: '$500',
              value: 500,
            },
          ],
        })}
      />
    )
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('Amounts')).toBeDefined()
    expect(screen.getByText('$500')).toBeDefined()
  })

  it('renders action items', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          action_items: ['Review the proposal', 'Send feedback by Friday'],
          ai_analyzed: true,
        })}
      />
    )
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('Action Items')).toBeDefined()
    expect(screen.getByText('Review the proposal')).toBeDefined()
    expect(screen.getByText('Send feedback by Friday')).toBeDefined()
  })

  it('does not render details section when no people/dates/amounts/actions', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          action_items: [],
          ai_analyzed: true,
          amounts: [],
          dates: [],
          people: [],
          summary: 'Just a summary',
        })}
      />
    )
    expect(screen.queryByText('People')).toBeNull()
    expect(screen.queryByText('Dates')).toBeNull()
    expect(screen.queryByText('Amounts')).toBeNull()
    expect(screen.queryByText('Action Items')).toBeNull()
  })
})
