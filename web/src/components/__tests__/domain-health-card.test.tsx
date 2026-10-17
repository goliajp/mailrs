import type { DomainCheckReport } from '@/lib/types'

import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { DomainHealthCard } from '../domain-health-card'

/** The shape `check_domain_dns` answers with (admin_ops/domain_check.rs). */
const REPORT: DomainCheckReport = {
  checked_at: 1_792_206_288,
  checks: [
    { details: ['10 mail.golia.ai'], message: '1 MX host(s)', name: 'MX', status: 'pass' },
    {
      details: ['v=spf1 mx ?all'],
      message: 'Record does not end in -all or ~all',
      name: 'SPF',
      status: 'warn',
    },
    {
      details: [],
      message: 'No DKIM key at mail._domainkey.golia.jp',
      name: 'DKIM',
      status: 'fail',
    },
    { details: [], message: 'No DMARC record', name: 'DMARC', status: 'fail' },
  ],
  domain: 'golia.jp',
}

describe('DomainHealthCard', () => {
  it('renders each check with its status', () => {
    render(<DomainHealthCard checking={false} onRecheck={vi.fn()} report={REPORT} />)
    for (const name of ['MX', 'SPF', 'DKIM', 'DMARC']) {
      expect(screen.getByText(name)).toBeInTheDocument()
    }
    expect(screen.getAllByText('Pass')).toHaveLength(1)
    expect(screen.getAllByText('Warn')).toHaveLength(1)
    expect(screen.getAllByText('Fail')).toHaveLength(2)
    expect(screen.getByText('1 pass')).toBeInTheDocument()
    expect(screen.getByText('1 warn')).toBeInTheDocument()
    expect(screen.getByText('2 fail')).toBeInTheDocument()
    expect(screen.getByText('No DMARC record')).toBeInTheDocument()
  })

  it('shows a check’s records when opened', () => {
    render(<DomainHealthCard checking={false} onRecheck={vi.fn()} report={REPORT} />)
    expect(screen.queryByText('v=spf1 mx ?all')).toBeNull()
    fireEvent.click(screen.getByText('SPF'))
    expect(screen.getByText('v=spf1 mx ?all')).toBeInTheDocument()
  })

  it('says when a check is running', () => {
    render(<DomainHealthCard checking onRecheck={vi.fn()} report={REPORT} />)
    expect(screen.getByRole('button', { name: /Running/ })).toBeDisabled()
  })
})
