/**
 * Narrowing the one list to some of the connected mailboxes.
 *
 * A **filter, not a switcher**: every box starts ticked and unticking
 * one takes that account out. Somebody with work, personal and two
 * others wants the first two together, which "only this" cannot say.
 *
 * The rule lives here rather than in the control because all three
 * clients follow it, and a filter that behaves differently on a phone
 * is a filter nobody trusts.
 */

/** What the control says it is doing. */
export function filterLabel(selected: null | string[], all: string[]): string {
  if (selected === null || selected.length === all.length) return 'All accounts'
  return `${selected.length} of ${all.length} accounts`
}

/**
 * The ids to ask for after ticking or unticking one.
 *
 * `null` means no filter at all: back to everything is not "every id
 * in the parameter", it is the parameter absent. A request carrying
 * every id narrows to the same set and costs a longer URL to say
 * nothing — and the two are indistinguishable in a log.
 *
 * Unticking the **last** one is refused rather than sending an empty
 * filter: a list narrowed to no accounts is a blank screen whose only
 * way back is the control that produced it.
 */
export function toggledAccounts(
  selected: null | string[],
  all: string[],
  id: string
): null | string[] {
  const on = selected ?? all
  const next = on.includes(id) ? on.filter((v) => v !== id) : [...on, id]
  if (next.length === 0) return selected
  return next.length === all.length ? null : next
}
