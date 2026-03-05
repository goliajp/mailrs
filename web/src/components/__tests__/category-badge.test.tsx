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
    expect(span?.className).toContain('text-red-700')
  })

  it('renders unknown category as-is', () => {
    render(<CategoryBadge category="custom-tag" />)
    expect(screen.getByText('custom-tag')).toBeDefined()
  })

  it('uses fallback styles for unknown category', () => {
    const { container } = render(<CategoryBadge category="unknown" />)
    const span = container.querySelector('span')
    expect(span?.className).toContain('bg-zinc-100')
    expect(span?.className).toContain('text-zinc-500')
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
  it('returns red for score >= 60', () => {
    expect(riskColor(60)).toBe('text-red-500')
    expect(riskColor(100)).toBe('text-red-500')
    expect(riskColor(75)).toBe('text-red-500')
  })

  it('returns amber for score 40-59', () => {
    expect(riskColor(40)).toBe('text-amber-500')
    expect(riskColor(59)).toBe('text-amber-500')
    expect(riskColor(50)).toBe('text-amber-500')
  })

  it('returns blue for score 15-39', () => {
    expect(riskColor(15)).toBe('text-blue-500')
    expect(riskColor(39)).toBe('text-blue-500')
    expect(riskColor(25)).toBe('text-blue-500')
  })

  it('returns green for score < 15', () => {
    expect(riskColor(0)).toBe('text-green-500')
    expect(riskColor(14)).toBe('text-green-500')
    expect(riskColor(1)).toBe('text-green-500')
  })

  it('handles boundary values exactly', () => {
    expect(riskColor(14)).toBe('text-green-500')
    expect(riskColor(15)).toBe('text-blue-500')
    expect(riskColor(39)).toBe('text-blue-500')
    expect(riskColor(40)).toBe('text-amber-500')
    expect(riskColor(59)).toBe('text-amber-500')
    expect(riskColor(60)).toBe('text-red-500')
  })
})
