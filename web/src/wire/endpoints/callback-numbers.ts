import type { WireCallbackNumbers } from '../schemas/callback-numbers'

import { wireFetch } from '../client'
import { callbackNumbersSchema } from '../schemas/callback-numbers'

/** The numbers `address` gave the signed-in reader before, earliest first. */
export function wireCallbackNumbers(address: string): Promise<WireCallbackNumbers> {
  return wireFetch(callbackNumbersSchema, {
    path: `/senders/${encodeURIComponent(address)}/callback-numbers`,
  })
}
