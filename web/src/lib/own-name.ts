import { tNow } from '@/i18n'

/** How a message's sender is shown: "Me" for the reader's own, else the name. */
export function ownName(isOwn: boolean, name: string): string {
  if (isOwn) return tNow('thread.me')
  return name
}
