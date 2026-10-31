import { toast } from '@goliapkg/gds'
import { useState } from 'react'

import { BottomSheet } from '@/components/bottom-sheet'
import { useEmptyTrashMutation } from '@/hooks/use-trash-mutations'
import { type Key, useT } from '@/i18n'
import { errorText } from '@/lib/error-text'

/**
 * The line above the Trash: how long it keeps things, and the one way
 * to delete everything in it now. Emptying is asked here and the server
 * may ask for the password again on top, since nothing brings it back.
 */
export function TrashBanner({ empty }: { empty: boolean }) {
  const t = useT()
  const [asking, setAsking] = useState(false)
  const emptyTrash = useEmptyTrashMutation()
  const confirm = () => {
    setAsking(false)
    emptyTrash.mutate(undefined, {
      onError: (err) => toast.error(errorText(err, t('trash.emptyFailed'))),
      onSuccess: (r) => toast.success(t(deletedLine(r.deleted), { n: r.deleted })),
    })
  }
  return (
    <div className="border-border text-fg-muted flex items-center gap-2 border-b px-3 py-2 text-xs">
      <span className="flex-1">{t('trash.note')}</span>
      <button
        className="text-danger hover:bg-danger/10 focus-visible:ring-danger/50 shrink-0 rounded-md px-2.5 py-1 font-medium transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:opacity-50"
        disabled={empty || emptyTrash.isPending}
        onClick={() => setAsking(true)}
        type="button"
      >
        {t('trash.empty')}
      </button>
      <BottomSheet onClose={() => setAsking(false)} open={asking}>
        <h3 className="text-fg text-sm font-semibold">{t('trash.confirmTitle')}</h3>
        <p className="text-fg-muted mt-1.5 text-sm">{t('trash.confirmBody')}</p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            className="border-border text-fg-secondary hover:bg-bg-secondary rounded-md border px-3 py-2 text-sm transition-colors"
            onClick={() => setAsking(false)}
            type="button"
          >
            {t('common.cancel')}
          </button>
          <button
            className="bg-danger focus-visible:ring-danger/50 rounded-md px-3 py-2 text-sm font-medium text-white transition-colors hover:opacity-90 focus-visible:ring-2 focus-visible:outline-none"
            onClick={confirm}
            type="button"
          >
            {t('trash.empty')}
          </button>
        </div>
      </BottomSheet>
    </div>
  )
}

function deletedLine(n: number): Key {
  if (n === 1) return 'trash.deleted.one'
  return 'trash.deleted.other'
}
