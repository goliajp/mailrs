import { wireFetch } from '../client'
import { acceptSendersSchema, type WireAcceptSenders } from '../schemas/senders'

/** Accept first contacts: they leave the New section, now and for mail to come. */
export function wireAcceptSenders(addresses: string[]): Promise<WireAcceptSenders> {
  return wireFetch(acceptSendersSchema, {
    body: { addresses },
    method: 'POST',
    path: '/senders/accept',
  })
}
