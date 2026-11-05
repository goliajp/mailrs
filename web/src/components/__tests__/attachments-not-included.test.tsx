import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { AttachmentsNotIncluded } from '@/components/attachments-not-included'
import { attachmentsNotIncluded } from '@/components/composer/start'

afterEach(cleanup)

const RETURNED = {
  bcc: '',
  body: 'see you on monday',
  cc: '',
  from: '',
  id: 7,
  replyToThreadId: null,
  scheduledAt: null,
  subject: 'お知らせ',
  to: 'erin@outside.example',
}

describe('a draft a restore returned without its attachments', () => {
  it('says which were left out and where the original is', () => {
    render(<AttachmentsNotIncluded names={['agenda.pdf', 'map.png']} />)
    expect(
      screen.getByText('Attachments not included — the original is in Sent: agenda.pdf, map.png')
    ).toBeTruthy()
  })

  it('says nothing for a draft that left nothing out', () => {
    const { container } = render(<AttachmentsNotIncluded names={[]} />)
    expect(container.textContent).toBe('')
    render(<AttachmentsNotIncluded names={undefined} />)
    expect(screen.queryByRole('note')).toBeNull()
  })

  it('reaches the composer only from a draft that carries the names', () => {
    const draft = { ...RETURNED, attachmentsNotIncluded: ['agenda.pdf', 'map.png'] }
    expect(attachmentsNotIncluded({ draft, kind: 'draft' })).toEqual(['agenda.pdf', 'map.png'])
    expect(attachmentsNotIncluded({ draft: RETURNED, kind: 'draft' })).toEqual([])
    expect(attachmentsNotIncluded({ kind: 'new' })).toEqual([])
  })
})
