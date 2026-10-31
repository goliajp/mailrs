import { toast } from '@goliapkg/gds'

import { invalidateMail } from '@/lib/mail-cache'
import { wireUndoRuleRun } from '@/wire/endpoints/rules'

/** Says what a run changed, with the undo the server keeps for seven days. */
export function runToast(run: { changed: number; failed: number; run_id: string }) {
  let text = `Changed ${run.changed} conversations`
  if (run.failed > 0) text += `, ${run.failed} could not be changed`
  toast.success(text, {
    action: {
      label: 'Undo',
      onClick: async () => {
        try {
          const undone = await wireUndoRuleRun(run.run_id)
          toast.success(`Put back ${undone.restored} conversations`)
        } catch {
          toast.error('Could not undo')
        } finally {
          void invalidateMail()
        }
      },
    },
  })
}

/** The actions a rule can take, besides putting a label on. */
export const PLAIN_ACTIONS: { label: string; value: string }[] = [
  { label: 'Archive', value: 'archive' },
  { label: 'Mark read', value: 'read' },
  { label: 'Star', value: 'star' },
  { label: 'Move to Junk', value: 'junk' },
  { label: 'Move to Trash', value: 'trash' },
]

export function errorText(err: unknown): string {
  if (err instanceof Error) return err.message
  return 'Something went wrong'
}
