import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { StorageNotice } from '@/components/storage-banner'
import { storageLevel } from '@/lib/storage-level'

afterEach(() => {
  cleanup()
})

const GB = 1024 * 1024 * 1024

describe('StorageNotice', () => {
  it('says nothing below 90% of the quota', () => {
    const { container } = render(
      <StorageNotice storage={{ quota_bytes: GB, used_bytes: 0.89 * GB }} />
    )
    expect(container.textContent).toBe('')
  })

  it('says the mailbox is filling up from 90%, and that imports stop when full', () => {
    render(<StorageNotice storage={{ quota_bytes: GB, used_bytes: 0.93 * GB }} />)
    expect(screen.getByRole('status').textContent).toBe(
      'Storage is 93% full. Once it is full, imports will fail'
    )
  })

  it('past the quota, says imports are refused and sending is not', () => {
    render(<StorageNotice storage={{ quota_bytes: GB, used_bytes: 1.2 * GB }} />)
    expect(screen.getByRole('status').textContent).toBe(
      'Storage is full. New imports are refused; sending is not affected'
    )
  })

  it('says nothing for an account with no quota, or before the usage is known', () => {
    expect(storageLevel({ quota_bytes: null, used_bytes: 5 * GB })).toBe('ok')
    expect(storageLevel(undefined)).toBe('ok')
    const { container } = render(
      <StorageNotice storage={{ quota_bytes: null, used_bytes: 5 * GB }} />
    )
    expect(container.textContent).toBe('')
  })
})
