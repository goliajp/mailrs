import { describe, expect, it } from 'vitest'

import { emptyEndpoint, manualEndpoints } from '../manual-endpoints'

const good = (host: string, port: string, protocol: string) => ({
  ...emptyEndpoint(protocol),
  host,
  port,
})

describe('server settings somebody typed in', () => {
  it('sends both endpoints when both are complete', () => {
    const out = manualEndpoints(good('imap.x.jp', '993', 'imap'), good('smtp.x.jp', '465', 'smtp'))
    expect(out).toEqual({
      incoming: { host: 'imap.x.jp', port: 993, protocol: 'imap', tls: 'implicit' },
      outgoing: { host: 'smtp.x.jp', port: 465, protocol: 'smtp', tls: 'implicit' },
    })
  })

  // `Number('')` is 0, not NaN — an empty box would otherwise be sent
  // as a real port of zero, and the server would refuse it with a
  // validation error rather than the form saying what is missing.
  it('refuses an empty port rather than sending zero', () => {
    expect(
      manualEndpoints(good('imap.x.jp', '', 'imap'), good('smtp.x.jp', '465', 'smtp'))
    ).toBeNull()
  })

  it('refuses a half-filled pair', () => {
    expect(manualEndpoints(good('', '993', 'imap'), good('smtp.x.jp', '465', 'smtp'))).toBeNull()
    expect(manualEndpoints(good('imap.x.jp', '993', 'imap'), good('', '465', 'smtp'))).toBeNull()
  })

  it('refuses a port outside the range', () => {
    for (const p of ['0', '65536', '-1', '99999']) {
      expect(manualEndpoints(good('h', p, 'imap'), good('s', '465', 'smtp'))).toBeNull()
    }
  })

  it('refuses something that is not a whole number', () => {
    for (const p of ['99.5', 'abc', '9 9', '1e3']) {
      expect(manualEndpoints(good('h', p, 'imap'), good('s', '465', 'smtp'))).toBeNull()
    }
  })

  it('keeps the protocol and the encryption as chosen', () => {
    const out = manualEndpoints(
      { host: 'pop.x.jp', port: '110', protocol: 'pop3', tls: 'starttls' },
      { host: 'smtp.x.jp', port: '587', protocol: 'smtp', tls: 'starttls' }
    )
    expect(out?.incoming.protocol).toBe('pop3')
    expect(out?.incoming.tls).toBe('starttls')
  })
})
