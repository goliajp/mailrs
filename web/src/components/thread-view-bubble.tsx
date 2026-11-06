/**
 * A button in the reading pane's header bar.
 *
 * `disabled` is a real prop now. The prev/next arrows faked it with
 * `pointer-events-none opacity-30`, which blocks the mouse and not the
 * keyboard — tab to the greyed arrow, press Enter, and it moved. It
 * also announced as an ordinary enabled button, and `opacity-30` on
 * `text-fg-muted` is below any readable contrast.
 */
export function HdrBtn({
  children,
  className,
  disabled,
  onClick,
  title,
}: {
  children: React.ReactNode
  className?: string
  disabled?: boolean
  onClick: () => void
  title: string
}) {
  return (
    <button
      className={`text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary focus-visible:ring-accent/50 flex h-7 w-7 items-center justify-center rounded-md transition-colors focus-visible:ring-2 focus-visible:outline-none disabled:pointer-events-none disabled:opacity-40 ${className ?? ''}`}
      disabled={disabled}
      onClick={onClick}
      title={title}
      type="button"
    >
      {children}
    </button>
  )
}

export function SmBtn({
  children,
  onClick,
  title,
}: {
  children: React.ReactNode
  onClick: () => void
  title: string
}) {
  return (
    <button
      className="text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary flex h-7 w-7 items-center justify-center rounded-md transition-all duration-150"
      onClick={onClick}
      title={title}
    >
      {children}
    </button>
  )
}
