/**
 * A table that scrolls sideways instead of squashing.
 *
 * `overflow-x-auto` alone does nothing here: every consumer's table is
 * `w-full`, and an auto-layout table asked for 100% of its container
 * shrinks to fit rather than overflowing. So on a phone a five-column
 * audit row got about 65px per column, the timestamps carried
 * `whitespace-nowrap` and won, and actor / target / detail collapsed to
 * roughly one character per line — rows eight lines tall, no scrollbar,
 * nothing to drag.
 *
 * The floor goes here rather than in each of the dozen pages that use
 * this: one of them had it (`admin-accounts`), which is how a shared
 * wrapper quietly stops being shared.
 */
export function ScrollableTable({ children }: { children: React.ReactNode }) {
  return (
    <div className="border-border overflow-x-auto rounded-lg border">
      <div className="min-w-[640px]">{children}</div>
    </div>
  )
}
