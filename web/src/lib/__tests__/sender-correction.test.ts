import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/lib/mail-cache', () => ({ findConversation: vi.fn(), invalidateMail: vi.fn() }))
vi.mock('@/wire/endpoints/sender-rules', () => ({
  wireReclassifySender: vi.fn(),
  wireUndoCorrection: vi.fn(),
}))
vi.mock('@goliapkg/gds', () => ({ toast: { error: vi.fn(), info: vi.fn(), success: vi.fn() } }))

import { toast } from '@goliapkg/gds'

import { findConversation } from '@/lib/mail-cache'
import { offerSenderCorrection, senderOf } from '@/lib/sender-correction'
import { wireReclassifySender, wireUndoCorrection } from '@/wire/endpoints/sender-rules'

afterEach(() => vi.clearAllMocks())

const row = (participants: string[]) =>
  ({ participants }) as unknown as ReturnType<typeof findConversation>

describe('sender correction', () => {
  it('the sender is the first participant who is not the reader', () => {
    vi.mocked(findConversation).mockReturnValue(row(['Me <me@x.test>', 'Shop <News@Shop.test>']))
    expect(senderOf('t1', 'me@x.test')).toBe('news@shop.test')
    vi.mocked(findConversation).mockReturnValue(null)
    expect(senderOf('t1', 'me@x.test')).toBeNull()
  })

  it('only offers; moving the sender and undoing it happen when asked', async () => {
    vi.mocked(findConversation).mockReturnValue(row(['news@shop.test']))
    vi.mocked(wireReclassifySender).mockResolvedValue({ correction_id: 'c1', moved: 3 })
    vi.mocked(wireUndoCorrection).mockResolvedValue({ restored: 3 })
    offerSenderCorrection('t1', 'mark-promotion', 'me@x.test')
    expect(wireReclassifySender).not.toHaveBeenCalled()
    const offer = vi.mocked(toast.info).mock.calls[0][1] as { action: { onClick: () => void } }
    offer.action.onClick()
    await vi.waitFor(() =>
      expect(wireReclassifySender).toHaveBeenCalledWith('news@shop.test', 'promotions')
    )
    await vi.waitFor(() => expect(toast.success).toHaveBeenCalled())
    const moved = vi.mocked(toast.success).mock.calls[0][1] as { action: { onClick: () => void } }
    moved.action.onClick()
    await vi.waitFor(() => expect(wireUndoCorrection).toHaveBeenCalledWith('c1'))
  })

  it('an action that is not a correction offers nothing', () => {
    vi.mocked(findConversation).mockReturnValue(row(['news@shop.test']))
    offerSenderCorrection('t1', 'archive', 'me@x.test')
    expect(toast.info).not.toHaveBeenCalled()
  })
})
