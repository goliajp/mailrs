import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SenderClaimBadge } from '../sender-claim-badge'

describe('SenderClaimBadge', () => {
  it('names the sending domain when the display name claims another', () => {
    const { container } = render(
      <SenderClaimBadge sender="Amazon.co.jp <x@mail07.jqjintaiyang.com>" />
    )
    expect(container.textContent).toContain('mail07.jqjintaiyang.com')
  })

  /** The vast ordinary middle stays unmarked. */
  it('renders nothing when the name and the sender agree', () => {
    const { container } = render(<SenderClaimBadge sender="Amazon.co.jp <x@amazon.co.jp>" />)
    expect(container.innerHTML).toBe('')
  })

  it('renders nothing when the name claims no domain', () => {
    const { container } = render(<SenderClaimBadge sender="Ann O'Brien <ann@example.com>" />)
    expect(container.innerHTML).toBe('')
  })
})
