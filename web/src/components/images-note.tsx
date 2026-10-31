const NOTE = 'border-border bg-bg-secondary text-fg-secondary border-b px-3 py-2 text-xs'

/** Above mail in Junk or held, whose images are not fetched unless asked. */
export function ImagesHeldNote({ onShow }: { onShow: () => void }) {
  return (
    <p className={NOTE}>
      Images are not loaded for mail in Junk or held for review: loading them tells the sender the
      address is read.{' '}
      <button className="text-accent hover:underline" onClick={onShow} type="button">
        Show images anyway
      </button>
    </p>
  )
}

/** Above a message whose images the settings keep from showing. */
export function ImagesHiddenNote({ mode, onShow }: { mode: string; onShow: () => void }) {
  if (mode === 'never') {
    return <p className={NOTE}>Images are hidden by your settings</p>
  }
  return (
    <p className={NOTE}>
      Images in this message are hidden.{' '}
      <button className="text-accent hover:underline" onClick={onShow} type="button">
        Show images
      </button>
    </p>
  )
}
