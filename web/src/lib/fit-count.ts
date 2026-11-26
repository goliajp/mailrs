/**
 * How many of `count` equal slots go inline in `width` px. When they do
 * not all fit, or `needsMore` says the overflow button is there anyway,
 * one slot is the button's.
 */
export function fitCount(
  width: number,
  count: number,
  slot: { gap: number; size: number },
  needsMore: boolean
): number {
  const fits = (n: number) => n * slot.size + Math.max(0, n - 1) * slot.gap <= width
  if (!needsMore && fits(count)) return count
  for (let n = count; n > 0; n--) if (fits(n + 1)) return n
  return 0
}
