import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { StatusBarView } from '../status-bar'

const BASE = {
  realtime: 'connected' as const,
  section: 'Mail',
  webVersion: '2026.07.07',
}

describe('<StatusBarView />', () => {
  it('renders both indicators with their labels visible', () => {
    render(<StatusBarView {...BASE} backend={{ status: 'healthy', version: '2.0.0' }} />)
    expect(screen.getByText('Backend')).toBeInTheDocument()
    expect(screen.getByText('Realtime')).toBeInTheDocument()
  })

  it('shows the current section', () => {
    render(<StatusBarView {...BASE} backend={{ status: 'healthy' }} section="Admin" />)
    expect(screen.getByText('Admin')).toBeInTheDocument()
  })

  it('renders the identity when provided', () => {
    render(<StatusBarView {...BASE} backend={{ status: 'healthy' }} identity="lihao@golia.jp" />)
    expect(screen.getByText('lihao@golia.jp')).toBeInTheDocument()
  })

  it('never shows "undefined" for the api version, even before the probe answers', () => {
    render(<StatusBarView {...BASE} backend={null} />)
    // "contacting…" is the deliberate placeholder for the api version.
    expect(screen.getByTestId('api-version')).toHaveTextContent('contacting…')
    // No stringified undefined leaks into the tree anywhere.
    expect(screen.queryByText(/undefined/i)).toBeNull()
  })

  it('renders the api version once it is present', () => {
    render(<StatusBarView {...BASE} backend={{ status: 'healthy', version: '2.0.0' }} />)
    expect(screen.getByTestId('api-version')).toHaveTextContent('2.0.0')
  })

  it('renders the injected web version verbatim', () => {
    render(<StatusBarView {...BASE} backend={{ status: 'healthy' }} webVersion="dev" />)
    expect(screen.getByTestId('web-version')).toHaveTextContent('dev')
  })

  it('renders pg / kevy check pills only when the backend reports those fields', () => {
    // Fastcore's /api/health doesn't include pg / kevy — the pills should
    // not be visible when the fields are absent.
    const { queryByText, rerender } = render(
      <StatusBarView {...BASE} backend={{ status: 'healthy' }} />
    )
    expect(queryByText(/^PG/)).toBeNull()

    rerender(<StatusBarView {...BASE} backend={{ kevy: true, pg: true, status: 'healthy' }} />)
    // The pg / kevy check pills are hidden below md breakpoint (jsdom's
    // default is a narrow viewport); assert via title on the wrapper span.
    expect(screen.getByText(/PG ✓/)).toBeInTheDocument()
    expect(screen.getByText(/Kevy ✓/)).toBeInTheDocument()
  })
})
