import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { AdminErrorState } from '@/components/admin-page/admin-error-state'
import { WireErrorException } from '@/wire/errors'

afterEach(cleanup)

describe('AdminErrorState', () => {
  it('shows a closed feature as a notice without a retry', () => {
    const error = new WireErrorException({ kind: 'disabled', message: 'Closed for security' })
    render(<AdminErrorState error={error} onRetry={() => {}} what="the account list" />)
    expect(screen.getByText('This feature is temporarily closed')).toBeTruthy()
    expect(screen.getByText('Closed for security')).toBeTruthy()
    expect(screen.queryByText('Try again')).toBeNull()
    expect(screen.queryByText(/Could not load/)).toBeNull()
  })

  it('keeps the failure and the retry for other errors', () => {
    const error = new WireErrorException({ kind: 'network' })
    render(<AdminErrorState error={error} onRetry={() => {}} what="the account list" />)
    expect(screen.getByText('Could not load the account list')).toBeTruthy()
    expect(screen.getByText('Try again')).toBeTruthy()
  })
})
