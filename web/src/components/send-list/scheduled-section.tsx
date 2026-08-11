import { toast } from '@goliapkg/gds'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Clock } from 'lucide-react'

import { formatFullDate } from '@/lib/format'
import { queryClient } from '@/lib/query-client'
import { wireCancelScheduled, wireListScheduled } from '@/wire/endpoints/sends'

const scheduledKey = ['sends', 'scheduled'] as const

/**
 * Mail that has been written and has not left yet.
 *
 * Above the sent rows, because this is the only screen that can stop a
 * scheduled message and it is worth nothing below fifty delivered
 * ones. `POST /api/scheduled/{id}/cancel` had existed since G13.3 with
 * no caller on any platform — nothing could list what there was to
 * cancel, so nothing could offer to.
 */
export function ScheduledSection() {
  const { data: items = [] } = useQuery({
    queryKey: scheduledKey,
    queryFn: () => wireListScheduled().then((rows) => [...rows]),
  })

  const cancel = useMutation({
    mutationFn: (id: string) => wireCancelScheduled(id),
    onError: (e: Error) => toast.error(e.message || 'Could not cancel that send'),
    onSuccess: () => {
      toast.success('Cancelled')
      void queryClient.invalidateQueries({ queryKey: scheduledKey })
    },
  })

  if (items.length === 0) return null

  return (
    <div className="border-border border-b">
      <h2 className="text-fg-muted px-3 pt-3 pb-1 text-xs font-medium tracking-wide uppercase">
        Scheduled
      </h2>
      {items.map((item) => (
        <div className="hover:bg-bg-secondary flex items-center gap-2 px-3 py-2" key={item.id}>
          <Clock className="text-fg-muted h-4 w-4 shrink-0" />
          <div className="min-w-0 flex-1">
            <div className="text-fg truncate text-sm">{item.subject || '(no subject)'}</div>
            <div className="text-fg-muted truncate text-xs">
              {item.recipient} · {formatFullDate(item.scheduledAt)}
            </div>
          </div>
          <button
            className="text-fg-muted hover:text-fg shrink-0 text-xs underline"
            disabled={cancel.isPending}
            onClick={() => cancel.mutate(item.id)}
          >
            Cancel
          </button>
        </div>
      ))}
    </div>
  )
}
