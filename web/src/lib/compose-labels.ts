import type { Key } from '@/i18n'

/** The send button's label, in the composer and the reply box. */
export function sendLabel(sending: boolean): Key {
  if (sending) return 'compose.sending'
  return 'compose.send'
}
