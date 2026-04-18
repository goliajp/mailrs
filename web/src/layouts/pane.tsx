// mailrs-specific pane wrappers that add the floating-panel visual style:
// raised background, rounded corners, and gaps between panels

import type { PaneGroupProps, PaneProps } from '@goliapkg/gds'

import { cx, Pane, PaneGroup } from '@goliapkg/gds'
import { forwardRef } from 'react'

export const MPane = forwardRef<HTMLDivElement, PaneProps>(function MPane(
  { className, ...props },
  ref
) {
  return (
    <Pane
      className={cx('bg-surface flex flex-col overflow-hidden md:rounded-lg', className)}
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
export const MPaneGroup = forwardRef<HTMLDivElement, PaneGroupProps>(function MPaneGroup(
  { className, ...props },
  ref
) {
  return <PaneGroup className={cx('min-w-0 gap-0 md:gap-1.5', className)} ref={ref} {...props} />
})
