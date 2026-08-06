/**
 * How much to shrink an email so it fits the width it is given.
 *
 * HTML email is authored against a fixed pixel width — a survey of 400
 * messages in this mailbox on 2026-08-05 found the declared widths
 * clustering at 600, 640, 650, 680, 700 and 768 px. None of those fit a
 * phone. Reflowing them is not on the table: the layout is tables and
 * absolute widths, and a client that reflows it shows something the
 * sender never composed.
 *
 * So the page is scaled, the way every mobile mail client scales it.
 * 600 px into a 366 px column is 0.61; 768 px is 0.48.
 */

/**
 * The floor exists to stop a pathological message — a 3000 px canvas, a
 * runaway `<pre>` — from being scaled into an unreadable smear. It is a
 * guard, not a policy: no width in that survey reaches it, so in practice
 * every real message fits exactly.
 *
 * Whatever the floor leaves over stays reachable by scrolling the body
 * sideways, which is why the container this renders into must not be
 * `overflow-hidden`.
 */
export const MIN_FIT_SCALE = 0.45

/**
 * The height the scaled message occupies.
 *
 * A transform does not change layout, so without this the container keeps
 * the unscaled height and leaves a band of blank space under short-looking
 * mail — the taller the message, the larger the gap.
 */
export function fitHeight(contentHeight: number, scale: number): number {
  if (!Number.isFinite(contentHeight) || contentHeight <= 0) return 0
  return contentHeight * scale
}

/**
 * `contentWidth` is the message's natural width, `hostWidth` the column
 * it has to live in.
 *
 * Never greater than 1: a narrow message is left at its own size rather
 * than blown up to fill a desktop pane, where the sender's 480 px design
 * stretched to 900 would look worse than the margin it was avoiding.
 */
export function fitScale(contentWidth: number, hostWidth: number): number {
  if (!Number.isFinite(contentWidth) || !Number.isFinite(hostWidth)) return 1
  if (contentWidth <= 0 || hostWidth <= 0) return 1
  if (contentWidth <= hostWidth) return 1
  return Math.max(MIN_FIT_SCALE, hostWidth / contentWidth)
}
