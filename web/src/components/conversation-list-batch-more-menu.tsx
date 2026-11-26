import { MoreVertical } from 'lucide-react'
import { useEffect, useRef } from 'react'

import { useT } from '@/i18n'
import { type ActionId, ACTIONS } from '@/lib/actions'
import { BATCH_ICON_BUTTON } from '@/lib/batch-bar-class'
import { rovingKeyDown } from '@/lib/roving'

/** The batch bar's actions that are not drawn inline, in a menu that opens upward. */
export function BatchMoreMenu({
  disabled,
  ids,
  onOpenChange,
  onRun,
  open,
}: {
  disabled: boolean
  ids: ActionId[]
  onOpenChange: (open: boolean) => void
  onRun: (id: ActionId) => void
  open: boolean
}) {
  const t = useT()
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const box = ref.current
    box?.querySelector<HTMLElement>('[role="menuitem"]')?.focus()
    const handle = (e: MouseEvent) => {
      if (box && !box.contains(e.target as Node)) onOpenChange(false)
    }
    document.addEventListener('mousedown', handle)
    return () => {
      document.removeEventListener('mousedown', handle)
      // closed from the keyboard: focus goes back to the button it left
      if (box?.contains(document.activeElement)) box.querySelector('button')?.focus()
    }
  }, [open, onOpenChange])

  return (
    <div className="relative shrink-0" ref={ref}>
      <button
        aria-expanded={open}
        aria-haspopup="menu"
        aria-label={t('batch.more')}
        className={`${BATCH_ICON_BUTTON} ${open ? 'bg-bg-secondary text-fg' : ''}`}
        disabled={disabled}
        onClick={() => onOpenChange(!open)}
        title={t('batch.more')}
        type="button"
      >
        <MoreVertical aria-hidden="true" className="h-[18px] w-[18px]" />
      </button>
      {open && (
        <div
          className="border-border bg-surface absolute right-0 bottom-full z-50 mb-2 min-w-[192px] rounded-lg border py-1 shadow-lg"
          onKeyDown={(e) => rovingKeyDown(e, '[role="menuitem"]', 'vertical')}
          role="menu"
        >
          {ids.map((id) => {
            const spec = ACTIONS[id]
            const Icon = spec.icon
            return (
              <button
                className="text-fg-secondary hover:bg-bg-secondary focus-visible:bg-bg-secondary flex w-full items-center gap-2.5 px-3 py-2 text-left text-sm whitespace-nowrap focus-visible:outline-none"
                data-batch-action={id}
                key={id}
                onClick={() => onRun(id)}
                role="menuitem"
                tabIndex={-1}
                type="button"
              >
                <Icon aria-hidden="true" className="h-4 w-4 shrink-0" />
                <span className="flex-1">{t(spec.label)}</span>
                {spec.keys[0] && (
                  <kbd className="text-fg-muted hidden pl-3 font-sans text-xs md:inline">
                    {spec.keys[0]}
                  </kbd>
                )}
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}
