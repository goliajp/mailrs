import { useAtom } from 'jotai'

import { MobileModal } from '@/components/mobile-modal'
import { useConversationActions } from '@/hooks/use-conversation-actions'
import { useT } from '@/i18n'
import { SNOOZE_CHOICES, snoozeSeconds } from '@/lib/snooze-choice'
import { snoozePickerAtom } from '@/store/ui'

/** Until when a conversation is put away: the four times iOS offers. */
export function SnoozePicker() {
  const t = useT()
  const [open, setOpen] = useAtom(snoozePickerAtom)
  const { snoozeUntil } = useConversationActions()
  if (!open) return null
  const close = () => setOpen(null)
  return (
    <MobileModal onClose={close} open>
      <div
        aria-label={t('snooze.title')}
        className="bg-surface border-border mx-4 w-full max-w-xs rounded-xl border p-2 shadow-xl"
        role="dialog"
      >
        <h3 className="text-fg px-3 pt-2 pb-1 text-sm font-semibold">{t('snooze.title')}</h3>
        {SNOOZE_CHOICES.map((c) => (
          <button
            className="text-fg hover:bg-bg-secondary focus-visible:bg-bg-secondary flex w-full rounded-lg px-3 py-2.5 text-left text-sm transition-colors focus-visible:outline-none"
            key={c.choice}
            onClick={() => {
              close()
              snoozeUntil(open.threadId, snoozeSeconds(c.choice, new Date()))
            }}
            type="button"
          >
            {t(c.label)}
          </button>
        ))}
      </div>
    </MobileModal>
  )
}
