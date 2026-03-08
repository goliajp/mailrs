import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'

import { CategoryBadge, riskColor } from '@/components/category-badge'

afterEach(() => {
  cleanup()
})

describe('CategoryBadge', () => {
  it('renders known category with correct label', () => {
    render(<CategoryBadge category="personal" />)
    expect(screen.getByText('Personal')).toBeDefined()
  })

  it('renders newsletter category', () => {
    render(<CategoryBadge category="newsletter" />)
    expect(screen.getByText('Newsletter')).toBeDefined()
  })

  it('renders scam category with red styles', () => {
    const { container } = render(<CategoryBadge category="scam" />)
    expect(screen.getByText('Scam')).toBeDefined()
    const span = container.querySelector('span')
    expect(span?.className).toContain('text-[var(--color-status-danger)]')
  })

  it('renders unknown category as-is', () => {
    render(<CategoryBadge category="custom-tag" />)
    expect(screen.getByText('custom-tag')).toBeDefined()
  })

  it('uses fallback styles for unknown category', () => {
    const { container } = render(<CategoryBadge category="unknown" />)
    const span = container.querySelector('span')
    expect(span?.className).toContain('bg-[var(--color-bg-raised)]')
    expect(span?.className).toContain('text-[var(--color-text-tertiary)]')
  })

  it('renders all known categories without error', () => {
    const categories = [
      'personal', 'general', 'notification', 'promotion', 'newsletter',
      'receipt', 'shipping', 'travel', 'finance', 'work', 'spam', 'scam',
    ]
    for (const cat of categories) {
      cleanup()
      const { container } = render(<CategoryBadge category={cat} />)
      expect(container.querySelector('span')?.textContent).toBeTruthy()
    }
  })
})

describe('riskColor', () => {
  it('returns danger for score >= 60', () => {
    expect(riskColor(60)).toBe('text-[var(--color-status-danger)]')
    expect(riskColor(100)).toBe('text-[var(--color-status-danger)]')
    expect(riskColor(75)).toBe('text-[var(--color-status-danger)]')
  })

  it('returns warning for score 40-59', () => {
    expect(riskColor(40)).toBe('text-[var(--color-status-warning)]')
    expect(riskColor(59)).toBe('text-[var(--color-status-warning)]')
    expect(riskColor(50)).toBe('text-[var(--color-status-warning)]')
  })

  it('returns info for score 15-39', () => {
    expect(riskColor(15)).toBe('text-[var(--color-status-info)]')
    expect(riskColor(39)).toBe('text-[var(--color-status-info)]')
    expect(riskColor(25)).toBe('text-[var(--color-status-info)]')
  })

  it('returns success for score < 15', () => {
    expect(riskColor(0)).toBe('text-[var(--color-status-success)]')
    expect(riskColor(14)).toBe('text-[var(--color-status-success)]')
    expect(riskColor(1)).toBe('text-[var(--color-status-success)]')
  })

  it('handles boundary values exactly', () => {
    expect(riskColor(14)).toBe('text-[var(--color-status-success)]')
    expect(riskColor(15)).toBe('text-[var(--color-status-info)]')
    expect(riskColor(39)).toBe('text-[var(--color-status-info)]')
    expect(riskColor(40)).toBe('text-[var(--color-status-warning)]')
    expect(riskColor(59)).toBe('text-[var(--color-status-warning)]')
    expect(riskColor(60)).toBe('text-[var(--color-status-danger)]')
  })
})
