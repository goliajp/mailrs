import type { ComposeRedraftSource } from '@/store/ui'

/**
 * The indices to keep, or `null` for "all of them".
 *
 * `null` and `[]` are different on the wire: absent keeps everything,
 * empty keeps nothing. Collapsing them would re-attach the files the user
 * removed, and they would only find out after sending.
 *
 * Returns `null` when there is nothing to select from, so a compose that
 * carries no attachments does not send an empty selection that reads like
 * a deliberate one.
 */
export function carriedSelection(
  redraft: ComposeRedraftSource | null,
  kept: ReadonlySet<number>
): null | number[] {
  if (!redraft || redraft.attachments.length === 0) return null
  return redraft.attachments.map((a) => a.index).filter((i) => kept.has(i))
}
