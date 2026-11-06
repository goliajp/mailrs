import type { Key } from '@/i18n'
import type { SwipeChoice, SwipeSetting } from '@/lib/swipe'

import { toast } from '@goliapkg/gds'

import { useSetSwipe, useSwipeSetting } from '@/hooks/use-swipe-setting'
import { useT } from '@/i18n'
import { SWIPE_CHOICES } from '@/lib/swipe'

import { cardClass, Field } from './_shared'
import { errorText } from './rules-shared'

const CHOICE_LABEL: Record<SwipeChoice, Key> = {
  archive: 'action.archive',
  none: 'settings.appearance.swipe.none',
  read: 'settings.appearance.swipe.read',
  trash: 'action.trash',
}

const SIDES: { label: Key; side: keyof SwipeSetting }[] = [
  { label: 'settings.appearance.swipe.right', side: 'right' },
  { label: 'settings.appearance.swipe.left', side: 'left' },
]

/** What a swipe on a row does, saved on the server for the phone apps too. */
export function SwipeCard() {
  const t = useT()
  const swipe = useSwipeSetting()
  const save = useSetSwipe()
  const choose = (side: keyof SwipeSetting, choice: SwipeChoice) =>
    save.mutate({ ...swipe, [side]: choice }, { onError: (e) => toast.error(errorText(e)) })
  return (
    <div className={cardClass}>
      <div className="space-y-3">
        <h3 className="text-sm font-medium">{t('settings.appearance.swipe')}</h3>
        {SIDES.map(({ label, side }) => (
          <Field key={side} label={t(label)}>
            <select
              aria-label={t(label)}
              className="border-border bg-surface rounded-md border px-2 py-1 text-sm"
              onChange={(e) => choose(side, e.target.value as SwipeChoice)}
              value={swipe[side]}
            >
              {SWIPE_CHOICES.map((c) => (
                <option key={c} value={c}>
                  {t(CHOICE_LABEL[c])}
                </option>
              ))}
            </select>
          </Field>
        ))}
        <p className="text-fg-muted text-xs">{t('settings.appearance.swipe.help')}</p>
      </div>
    </div>
  )
}
