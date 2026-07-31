/**
 * The shape of a row in any mail list.
 *
 * The Inbox and the Send view are separate components with separate data,
 * and each wrote its own row classes. They drifted: the conversation row
 * marks the selected one with an accent left border and a tinted
 * background, and the Send row had neither — so on the Send list nothing
 * showed which message the pane on the right was displaying.
 *
 * One definition, because "what a selected row looks like" is one fact
 * about the product and not two facts about two components.
 */

/** The invariant part — height, layout, focus ring. */
export const MAIL_ROW_BASE =
  'focus-visible:ring-accent/50 relative flex h-16 w-full items-start gap-3 overflow-hidden border-l-[3px] px-4 py-2 text-left transition-all duration-150 focus-visible:ring-2 focus-visible:outline-none'

export type MailRowState = {
  /** Batch mode suppresses the selected treatment; the checkbox carries it. */
  readonly batchMode?: boolean
  /** Ticked in batch mode. */
  readonly checked?: boolean
  /** Wants attention — a failed send, currently. Overrides the accent. */
  readonly flagged?: boolean
  /** Dim rows that have been read. */
  readonly muted?: boolean
  /** The row whose content the reading pane is showing. */
  readonly selected?: boolean
}

/** Layout plus state, for a row that is itself the clickable element. */
export function mailRowClass(state: MailRowState): string {
  return `${MAIL_ROW_BASE} ${mailRowStateClass(state)}`
}

/**
 * Just the state part — border, background, dimming.
 *
 * Separate from the layout so a row with its own internal structure can
 * share how a state *looks* without inheriting a flex box it does not want.
 * The drafts row is a wrapper with its own button inside; making it take
 * the full base meant overriding half of it back with `!important`, which
 * is worse than not sharing at all.
 *
 * `flagged` wins the left border: a send that failed is worth more of the
 * user's attention than which row happens to be open, and the two never
 * need to be distinguished at once.
 */
export function mailRowStateClass(state: MailRowState): string {
  const isSelected = Boolean(state.selected) && !state.batchMode
  const border = state.flagged
    ? 'border-l-danger/60'
    : isSelected
      ? 'border-l-accent'
      : 'border-l-transparent'
  const bg = isSelected || state.checked ? 'bg-accent/10' : 'hover:bg-bg-secondary'
  const dim = state.muted && !isSelected && !state.checked ? 'opacity-70 hover:opacity-100' : ''
  return `${border} ${bg} ${dim}`
}
