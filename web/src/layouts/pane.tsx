// mailrs-specific pane wrappers: panes sit edge to edge on the surface
// colour, with a hairline between neighbours, against the darker band of
// the icon rail

import type { PaneGroupProps, PaneProps } from '@goliapkg/gds'

import { cx, Pane, PaneGroup } from '@goliapkg/gds'
import { forwardRef } from 'react'

export const MPane = forwardRef<HTMLDivElement, PaneProps>(function MPane(
  { className, ...props },
  ref
) {
  return (
    <Pane
      className={cx('bg-surface flex flex-col overflow-hidden', className)}
      ref={ref}
      {...props}
    />
  )
})

// GDS PaneGroup ships with `min-h-0` but no `min-w-0`, so a flex child whose
// content has a wide intrinsic min (e.g. an email iframe rendered at the
// content's natural pixel width before transform-scale) can grow past its
// share, push the fixed-width sibling pane off screen, and overflow the page
// horizontally. forcing `min-w-0` everywhere lets flex shrink properly.
// `divide-x` draws the hairline on the right of every pane but the last; a
// hidden pane draws none.
export const MPaneGroup = forwardRef<HTMLDivElement, PaneGroupProps>(function MPaneGroup(
  { className, ...props },
  ref
) {
  return (
    <PaneGroup
      className={cx('divide-border min-w-0 md:divide-x', className)}
      ref={ref}
      {...props}
    />
  )
})
