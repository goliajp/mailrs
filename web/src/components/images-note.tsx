import { useT } from '@/i18n'
const NOTE = 'border-border bg-bg-secondary text-fg-secondary border-b px-3 py-2 text-xs'

/** Above mail in Junk or held, whose images are not fetched unless asked. */
export function ImagesHeldNote({ onShow }: { onShow: () => void }) {
  const t = useT()
  return (
    <p className={NOTE}>
      {t('images.heldNote')}{' '}
      <button className="text-accent hover:underline" onClick={onShow} type="button">
        {t('images.showAnyway')}
      </button>
    </p>
  )
}

/** Above a message whose images the settings keep from showing. */
export function ImagesHiddenNote({
  mode,
  onShow,
  onTrust,
  sender,
}: {
  mode: string
  onShow: () => void
  onTrust?: () => void
  sender?: string
}) {
  const t = useT()
  if (mode === 'never') {
    return <p className={NOTE}>{t('images.hiddenBySettings')}</p>
  }
  return (
    <p className={NOTE}>
      {t('images.hidden')}{' '}
      <button className="text-accent hover:underline" onClick={onShow} type="button">
        {t('images.show')}
      </button>
      {onTrust && sender && (
        <>
          {' · '}
          <button className="text-accent hover:underline" onClick={onTrust} type="button">
            {t('images.alwaysFrom', { sender })}
          </button>
        </>
      )}
    </p>
  )
}
