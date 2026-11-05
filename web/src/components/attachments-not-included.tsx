import { useT } from '@/i18n'

/**
 * A draft an account's restore returned names the attachments the message
 * had: a draft cannot carry them, and the original, with them, is in Sent.
 * Nothing when there were none.
 */
export function AttachmentsNotIncluded({
  className,
  names,
}: {
  className?: string
  names: readonly string[] | undefined
}) {
  const t = useT()
  if (!names || names.length === 0) return null
  return (
    <span className={`text-warning ${className ?? ''}`} role="note">
      {t('drafts.attachmentsNotIncluded', { names: names.join(', ') })}
    </span>
  )
}
