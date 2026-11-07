import { useT } from '@/i18n'

type Refusal = { readonly key: string; readonly name: string; readonly reason: string }

const NONE: readonly Refusal[] = []

/**
 * Says that the draft is not being saved, once retrying has stopped being a
 * plausible explanation. Rendered by both draft composers; see
 * `use-autosave-status` for why the previous silence was wrong.
 *
 * A file the server refused to keep with the draft is said at once, with
 * the server's reason: retrying the same file would be refused the same way.
 */
export function AutosaveWarning({
  error,
  refused = NONE,
  show,
}: {
  error: null | string
  refused?: readonly Refusal[]
  show: boolean
}) {
  const t = useT()
  if (!show && refused.length === 0) return null
  return (
    <div
      className="border-t border-amber-500/40 bg-amber-500/10 px-4 py-2 text-xs text-amber-700 dark:text-amber-300"
      role="status"
    >
      {show && (
        <p>
          <span className="font-medium">{t('autosave.title')}</span>{' '}
          <span>{t('autosave.body')}</span>
          {error !== null && <span className="mt-0.5 block opacity-70">{error}</span>}
        </p>
      )}
      {refused.length > 0 && (
        <p>
          <span className="font-medium">{t('autosave.fileTitle')}</span>{' '}
          <span>{t('autosave.fileBody')}</span>
          {refused.map((r) => (
            <span className="mt-0.5 block opacity-70" key={r.key}>
              {r.name}: {r.reason}
            </span>
          ))}
        </p>
      )}
    </div>
  )
}
