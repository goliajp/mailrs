// Importing an mbox from settings: the chosen file goes up as the request
// body, and what the server did is shown, failures and a cut-off upload in
// their own lines.

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/mail-import', () => ({ wireImportMbox: vi.fn() }))
vi.mock('@/lib/mail-cache', () => ({ invalidateMail: vi.fn() }))

import { MboxImportCard } from '@/components/settings/mbox-import-card'
import { invalidateMail } from '@/lib/mail-cache'
import { wireImportMbox } from '@/wire/endpoints/mail-import'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const REPORT = {
  archived: 1,
  complete: true,
  failed: 0,
  imported: 3,
  sent: 1,
  skipped_duplicate: 2,
  skipped_junk_or_trash: 1,
  too_large: 0,
}

function choose(file: File) {
  fireEvent.change(screen.getByLabelText('MBOX file'), { target: { files: [file] } })
}

describe('mbox import', () => {
  it('uploads the chosen file and says what it did', async () => {
    vi.mocked(wireImportMbox).mockResolvedValue(REPORT)
    render(<MboxImportCard />)
    const file = new File(['From x\n\nb\n'], 'takeout.mbox')
    choose(file)
    await vi.waitFor(() => expect(wireImportMbox).toHaveBeenCalledWith(file))
    expect(
      await screen.findByText('Imported 3 into the Inbox (1 of them archived) and 1 into Sent')
    ).toBeDefined()
    expect(screen.getByText('Skipped 2 already here and 1 from Junk or Trash')).toBeDefined()
    expect(screen.queryByText(/could not be filed/)).toBeNull()
    expect(invalidateMail).toHaveBeenCalled()
  })

  it('says when messages failed or the upload stopped early', async () => {
    vi.mocked(wireImportMbox).mockResolvedValue({
      ...REPORT,
      complete: false,
      failed: 2,
      too_large: 1,
    })
    render(<MboxImportCard />)
    choose(new File(['x'], 'a.mbox'))
    expect(await screen.findByText('2 could not be filed and 1 were too large')).toBeDefined()
    expect(screen.getByText(/The upload stopped before the end of the file/)).toBeDefined()
  })
})
