import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'

import type { ThreadMessage } from '@/lib/types'
import { AiAnalysisPanel } from '@/components/ai-analysis'

afterEach(() => {
  cleanup()
})

function makeMessage(overrides: Partial<ThreadMessage> = {}): ThreadMessage {
  return {
    id: 1,
    uid: 100,
    sender: 'alice@example.com',
    recipients: 'bob@example.com',
    subject: 'Test',
    flags: 0,
    internal_date: 1700000000,
    message_id: '<msg1@example.com>',
    text_body: 'text',
    html_body: null,
    attachments: [],
    category: 'general',
    risk_score: 0,
    risk_reason: '',
    summary: '',
    people: [],
    dates: [],
    amounts: [],
    action_items: [],
    ai_analyzed: false,
    clean_text: null,
    new_content: null,
    importance_level: 'normal',
    importance_score: 0.3,
    is_bulk_sender: false,
    has_tracking_pixel: false,
    requires_action: false,
    sender_intent: 'inform',
    action_deadline: null,
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
        message={makeMessage({ ai_analyzed: true, summary: 'This email is about a meeting.' })}
      />,
    )
    expect(screen.getByText('This email is about a meeting.')).toBeDefined()
  })

  it('renders risk reason when risk_score > 0', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          ai_analyzed: true,
          risk_score: 50,
          risk_reason: 'Contains suspicious links',
        })}
      />,
    )
    expect(screen.getByText(/Contains suspicious links/)).toBeDefined()
  })

  it('does not render risk reason when risk_score is 0', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          ai_analyzed: true,
          risk_score: 0,
          risk_reason: 'Should not show',
        })}
      />,
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
      />,
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
          dates: [{ text: 'March 15', context: 'deadline', iso_date: '2025-03-15' }],
        })}
      />,
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
          amounts: [{ text: '$500', value: 500, currency: 'USD', context: 'invoice total' }],
        })}
      />,
    )
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('Amounts')).toBeDefined()
    expect(screen.getByText('$500')).toBeDefined()
  })

  it('renders action items', () => {
    render(
      <AiAnalysisPanel
        message={makeMessage({
          ai_analyzed: true,
          action_items: ['Review the proposal', 'Send feedback by Friday'],
        })}
      />,
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
          ai_analyzed: true,
          summary: 'Just a summary',
          people: [],
          dates: [],
          amounts: [],
          action_items: [],
        })}
      />,
    )
    expect(screen.queryByText('People')).toBeNull()
    expect(screen.queryByText('Dates')).toBeNull()
    expect(screen.queryByText('Amounts')).toBeNull()
    expect(screen.queryByText('Action Items')).toBeNull()
  })
})
