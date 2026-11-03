/**
 * When the list asks for its next page.
 *
 * It used to ask when a sentinel after the last row came within 300 px of
 * the viewport. The sentinel only exists while the virtualizer renders it
 * (ten rows of overscan, 640 px), so no margin could ask earlier than
 * that, and a reader scrolling at an ordinary pace reached the end of the
 * rows with the page still a round trip away: on a throttled Fast 4G link
 * that wait measured 333 ms at the median. Asking while the last visible
 * row is still `LOAD_AHEAD_ROWS` from the end hides the round trip behind
 * the scrolling.
 */

/** Rows, about two screens of the list, still to scroll through when the next page is asked for. */
export const LOAD_AHEAD_ROWS = 25

/**
 * Whether the last visible item (`visibleEnd`, an index into the `count`
 * items the list draws) is within `LOAD_AHEAD_ROWS` of the end. Nothing
 * visible yet (`-1`) is not near anything.
 */
export function nearEnd(visibleEnd: number, count: number): boolean {
  if (visibleEnd < 0) return false
  return visibleEnd >= count - 1 - LOAD_AHEAD_ROWS
}
