import type { WireDeletedNote } from '@/wire/schemas/shared'

import { toast } from '@goliapkg/gds'
import { useState } from 'react'

import { BottomSheet } from '@/components/bottom-sheet'
import { useT } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { formatFullDate } from '@/lib/format'
import { parseTeamConv, wireReadDeletedNote } from '@/wire/endpoints/shared'

/**
 * On an audit row recording a deleted team note: read what it said. The
 * server records this read in the audit log before answering, and nothing
 * else shows a deleted note's text.
 */
export function DeletedNoteButton({
  action,
  detail,
  target,
}: {
  action: string
  detail: string
  target: string
}) {
  const t = useT()
  const [note, setNote] = useState<null | WireDeletedNote>(null)
  const c = parseTeamConv(target)
  const id = /^note (\d+)$/.exec(detail)?.[1]
  if (action !== 'shared_note.delete' || !c || !id) return null
  const read = async () => {
    try {
      setNote(await wireReadDeletedNote(c, Number(id)))
    } catch (e) {
      toast.error(errorText(e, t('team.deleted.failed')))
    }
  }
  return (
    <>
      <button
        className="text-accent text-xs hover:opacity-80"
        onClick={() => void read()}
        type="button"
      >
        {t('team.deleted.read')}
      </button>
      <BottomSheet onClose={() => setNote(null)} open={note !== null}>
        <h3 className="text-fg text-sm font-semibold">{t('team.deleted.title')}</h3>
        <p className="text-fg-muted mt-1 text-xs">{t('team.deleted.recorded')}</p>
        {note && (
          <div className="mt-3 space-y-2 text-sm">
            <p className="text-fg-muted text-xs">
              {t('team.deleted.by', {
                author: note.by,
                deleter: note.deleted_by,
                time: formatFullDate(note.deleted_at),
              })}
            </p>
            {note.versions.map((v, i) => (
              <pre className="bg-bg-secondary rounded-md p-2 text-xs whitespace-pre-wrap" key={i}>
                {v}
              </pre>
            ))}
          </div>
        )}
      </BottomSheet>
    </>
  )
}
