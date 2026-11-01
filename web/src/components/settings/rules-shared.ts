import { toast } from '@goliapkg/gds'

import { type Key, tNow as now } from '@/i18n'
import { invalidateMail } from '@/lib/mail-cache'
import { wireUndoRuleRun } from '@/wire/endpoints/rules'

/** Says what a run changed, with the undo the server keeps for seven days. */
export function runToast(run: { changed: number; failed: number; run_id: string }) {
  let text = now('settings.rules.changed', { count: run.changed })
  if (run.failed > 0) text += now('settings.rules.someFailed', { count: run.failed })
  toast.success(text, {
    action: {
      label: now('common.undo'),
      onClick: async () => {
        try {
          const undone = await wireUndoRuleRun(run.run_id)
          toast.success(now('settings.rules.putBack', { count: undone.restored }))
        } catch {
          toast.error(now('settings.rules.undoFailed'))
        } finally {
          void invalidateMail()
        }
      },
    },
  })
}

/** The actions a rule can take, besides putting a label on. */
export const PLAIN_ACTIONS: { label: Key; value: string }[] = [
  { label: 'settings.rules.action.archive', value: 'archive' },
  { label: 'settings.rules.action.read', value: 'read' },
  { label: 'settings.rules.action.star', value: 'star' },
  { label: 'settings.rules.action.junk', value: 'junk' },
  { label: 'settings.rules.action.trash', value: 'trash' },
]

export function errorText(err: unknown): string {
  if (err instanceof Error) return err.message
  return now('settings.somethingWrong')
}
