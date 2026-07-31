/**
 * Says that the draft is not being saved, once retrying has stopped being a
 * plausible explanation. Rendered by both draft composers; see
 * `use-autosave-status` for why the previous silence was wrong.
 */
export function AutosaveWarning({ error, show }: { error: null | string; show: boolean }) {
  if (!show) return null
  return (
    <div
      className="border-t border-amber-500/40 bg-amber-500/10 px-4 py-2 text-xs text-amber-700 dark:text-amber-300"
      role="status"
    >
      <span className="font-medium">Draft not saved.</span>{' '}
      <span>
        The last few attempts failed, so this text exists only in this window — copy it before
        closing.
      </span>
      {error === null ? null : <span className="mt-0.5 block opacity-70">{error}</span>}
    </div>
  )
}
