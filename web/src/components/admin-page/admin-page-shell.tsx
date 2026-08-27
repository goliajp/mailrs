import { useId } from 'react'

type AdminPageShellProps = {
  actions?: React.ReactNode
  children: React.ReactNode
  subtitle?: string
  title: string
}

/**
 * The frame every admin page sits in.
 *
 * Two things it did not do. It had **no maximum width**, so on a wide
 * monitor the tables ran to two thousand points while Settings, which
 * caps at `max-w-2xl`, stayed a readable column — the same app looking
 * like two products. And it had no place for a subtitle, so the two
 * pages that wanted one pulled it up under the heading with `-mt-4`
 * and landed on different margins.
 *
 * The header also wraps now: it was a bare `flex` holding a title, a
 * 256px filter box and a button, which on a 375px screen is about
 * 450px of content in 327px of room.
 */
export function AdminPageShell({ actions, children, subtitle, title }: AdminPageShellProps) {
  const titleId = useId()
  return (
    <main aria-labelledby={titleId} className="flex-1 overflow-y-auto p-6" role="region">
      <div className="mx-auto w-full max-w-6xl">
        <div className="mb-6 flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0">
            <h2 className="truncate text-lg font-semibold" id={titleId}>
              {title}
            </h2>
            {subtitle && <p className="text-fg-secondary mt-1 text-sm">{subtitle}</p>}
          </div>
          {actions && <div className="flex min-w-0 flex-wrap items-center gap-2">{actions}</div>}
        </div>
        {children}
      </div>
    </main>
  )
}
