import { describe, expect, it } from 'vitest'

import {
  avatarColor,
  avatarInitial,
  decodeMimeHeader,
  extractEmail,
  extractName,
  isMachineGenerated,
} from '../avatar'

// valid Tailwind color classes used in avatar.ts
const VALID_COLORS = [
  'bg-red-500',
  'bg-orange-500',
  'bg-amber-500',
  'bg-yellow-500',
  'bg-lime-500',
  'bg-green-500',
  'bg-emerald-500',
  'bg-teal-500',
  'bg-cyan-500',
  'bg-sky-500',
  'bg-blue-500',
  'bg-indigo-500',
  'bg-violet-500',
  'bg-purple-500',
  'bg-fuchsia-500',
  'bg-pink-500',
]

describe('avatarColor', () => {
  it('returns a valid color class for a normal email', () => {
    const result = avatarColor('alice@example.com')
    expect(VALID_COLORS).toContain(result)
  })

  it('returns a valid color class for an empty string', () => {
    const result = avatarColor('')
    expect(VALID_COLORS).toContain(result)
  })

  it('returns the same color for the same email (deterministic)', () => {
    const email = 'bob@example.com'
    expect(avatarColor(email)).toBe(avatarColor(email))
  })

  it('may return different colors for different emails', () => {
    // not strictly guaranteed, but with 16 buckets two arbitrary distinct
    // emails usually hash differently — collect 8 and expect at least 2 distinct
    const emails = [
      'a@example.com',
      'b@example.com',
      'c@example.com',
      'd@example.com',
      'e@example.com',
      'f@example.com',
      'g@example.com',
      'h@example.com',
    ]
    const colors = new Set(emails.map(avatarColor))
    expect(colors.size).toBeGreaterThan(1)
  })

  it('handles unicode characters without throwing', () => {
    expect(() => avatarColor('用户@例子.中国')).not.toThrow()
  })
})

describe('avatarInitial', () => {
  it('returns the first letter of the display name when in "Name <email>" format', () => {
    expect(avatarInitial('Alice Smith <alice@example.com>')).toBe('A')
  })

  it('returns uppercase initial', () => {
    expect(avatarInitial('bob jones <bob@example.com>')).toBe('B')
  })

  it('handles quoted display name', () => {
    expect(avatarInitial('"Charlie Brown" <charlie@example.com>')).toBe('C')
  })

  it('returns first char of email when no display name', () => {
    expect(avatarInitial('dave@example.com')).toBe('D')
  })

  it('returns uppercase for lowercase-starting email', () => {
    expect(avatarInitial('eve@example.com')).toBe('E')
  })

  it('returns "?" for an empty string', () => {
    expect(avatarInitial('')).toBe('?')
  })

  it('handles display name with leading whitespace', () => {
    // "  Frank <frank@example.com>" — trim() should strip leading space
    expect(avatarInitial('  Frank <frank@example.com>')).toBe('F')
  })
})

describe('extractEmail', () => {
  it('extracts email from "Name <email>" format', () => {
    expect(extractEmail('Alice Smith <alice@example.com>')).toBe('alice@example.com')
  })

  it('extracts email from bare angle-bracket format', () => {
    expect(extractEmail('<bob@example.com>')).toBe('bob@example.com')
  })

  it('returns the input as-is when no angle brackets present', () => {
    expect(extractEmail('charlie@example.com')).toBe('charlie@example.com')
  })

  it('returns the input when format is unrecognised', () => {
    expect(extractEmail('not-an-email')).toBe('not-an-email')
  })

  it('handles quoted display name', () => {
    expect(extractEmail('"Dave Doe" <dave@example.com>')).toBe('dave@example.com')
  })

  it('handles subdomain emails', () => {
    expect(extractEmail('Eve <eve@mail.example.co.uk>')).toBe('eve@mail.example.co.uk')
  })
})

describe('extractName', () => {
  it('extracts display name from "Name <email>" format', () => {
    expect(extractName('Alice Smith <alice@example.com>')).toBe('Alice Smith')
  })

  it('strips surrounding quotes from quoted display name', () => {
    // regex captures everything between optional leading " and <, then trim
    expect(extractName('"Bob Jones" <bob@example.com>')).toBe('Bob Jones')
  })

  it('falls back to local part of email when no display name', () => {
    expect(extractName('charlie@example.com')).toBe('charlie')
  })

  it('falls back to full string as local part when no @ present', () => {
    expect(extractName('notanemail')).toBe('notanemail')
  })

  it('trims whitespace from extracted name', () => {
    expect(extractName('  Dave  <dave@example.com>')).toBe('Dave')
  })

  it('handles single-word display name', () => {
    expect(extractName('Eve <eve@example.com>')).toBe('Eve')
  })

  it('returns domain-based name for machine-generated bounce addresses', () => {
    const result = extractName(
      '<0101019ccdad21d4-3b587183-7366-4b5f-a157-c01ce00c45e1-000000@atlassian-bounces.atlassian.net>',
    )
    expect(result).toBe('Atlassian')
  })

  it('returns domain-based name for long hex local parts', () => {
    const result = extractName('01234567890abcdef01234567@mailer.example.com')
    expect(result).toBe('Example')
  })

  it('returns domain-based name for digit-heavy tracking IDs', () => {
    const result = extractName(
      '722-YFB-855.0.848.0.0.2733.9.9404639 <722-YFB-855.0.848.0.0.2733.9.9404639@magazine2.zehitomo.com>',
    )
    expect(result).toBe('Zehitomo')
  })

  it('returns domain-based name for VERP bounce addresses', () => {
    const result = extractName(
      'msprvs1=205268vVROh3y=bounces-265094 <msprvs1=205268vVROh3y=bounces-265094@notify.cloudflare.com>',
    )
    expect(result).toBe('Cloudflare')
  })

  it('returns domain-based name for bounce+ prefixed addresses', () => {
    const result = extractName(
      'bounce+6a5245.e953aa-lihao=golia.jp <bounce+6a5245.e953aa-lihao=golia.jp@crates.io>',
    )
    expect(result).toBe('Crates')
  })

  it('returns domain-based name for bounces+ with subdomain', () => {
    const result = extractName(
      'bounces+46507342-d0da-lihao=golia.jp <bounces+46507342-d0da-lihao=golia.jp@em8742.bsm.freee.work>',
    )
    expect(result).toBe('Freee')
  })

  it('returns domain-based name for tracking-id display names', () => {
    const result = extractName(
      'z-bd148-xiacv7-0-2t4n-005lihaogolia.jp <z-bd148-xiacv7-0-2t4n-005lihaogolia.jp@bma.mpse.jp>',
    )
    expect(result).toBe('Mpse')
  })

  it('returns domain-based name for tracking-id with deeper subdomain', () => {
    const result = extractName(
      'z-cap7-xiadwo-0-5piy-005lihaogolia.jp <z-cap7-xiadwo-0-5piy-005lihaogolia.jp@bma.rec.mpse.jp>',
    )
    expect(result).toBe('Mpse')
  })

  it('handles .co.jp multi-part TLD', () => {
    const result = extractName('0101019ccdad21d4-abcdef@bounce.example.co.jp')
    expect(result).toBe('Example')
  })

  it('keeps short normal local parts as-is', () => {
    expect(extractName('noreply@github.com')).toBe('noreply')
  })

  it('decodes MIME base64 encoded display name', () => {
    expect(extractName('=?UTF-8?B?6aKG6Iux?= <notifications-noreply@linkedin.com>')).toBe('领英')
  })

  it('decodes MIME quoted-printable display name', () => {
    expect(extractName('=?UTF-8?Q?Caf=C3=A9?= <cafe@example.com>')).toBe('Café')
  })
})

describe('isMachineGenerated', () => {
  it('detects long hex/uuid strings', () => {
    expect(isMachineGenerated('0101019ccdad21d4-3b587183-7366-4b5f-a157-c01ce00c45e1-000000')).toBe(
      true,
    )
  })

  it('detects digit-heavy tracking IDs', () => {
    expect(isMachineGenerated('722-YFB-855.0.848.0.0.2733.9.9404639')).toBe(true)
  })

  it('detects VERP bounce addresses', () => {
    expect(isMachineGenerated('msprvs1=205268vVROh3y=bounces-265094')).toBe(true)
  })

  it('detects bounce+ prefixed addresses', () => {
    expect(isMachineGenerated('bounce+6a5245.e953aa-lihao=golia.jp')).toBe(true)
  })

  it('detects bounces+ prefixed addresses', () => {
    expect(isMachineGenerated('bounces+46507342-d0da-lihao=golia.jp')).toBe(true)
  })

  it('detects long no-space strings with digits (tracking IDs)', () => {
    expect(isMachineGenerated('z-bd148-xiacv7-0-2t4n-005lihaogolia.jp')).toBe(true)
    expect(isMachineGenerated('z-cap7-xiadwo-0-5piy-005lihaogolia.jp')).toBe(true)
  })

  it('detects VERP = encoding in long strings', () => {
    expect(isMachineGenerated('user=recipient@sender.com')).toBe(true)
  })

  it('returns false for short human names', () => {
    expect(isMachineGenerated('alice')).toBe(false)
    expect(isMachineGenerated('noreply')).toBe(false)
    expect(isMachineGenerated('User123')).toBe(false)
  })

  it('returns false for normal display names', () => {
    expect(isMachineGenerated('Alice Smith')).toBe(false)
    expect(isMachineGenerated('Bob Jones')).toBe(false)
  })

  it('returns false for newsletter-style local parts', () => {
    expect(isMachineGenerated('newsletter')).toBe(false)
  })
})

describe('decodeMimeHeader', () => {
  it('passes through plain ASCII', () => {
    expect(decodeMimeHeader('Hello World')).toBe('Hello World')
  })

  it('decodes UTF-8 base64', () => {
    expect(decodeMimeHeader('=?UTF-8?B?5pel5pys6Kqe?=')).toBe('日本語')
  })

  it('decodes UTF-8 quoted-printable', () => {
    expect(decodeMimeHeader('=?UTF-8?Q?Caf=C3=A9?=')).toBe('Café')
  })

  it('handles multiple encoded-words', () => {
    const result = decodeMimeHeader('=?UTF-8?B?5pel5pys?= =?UTF-8?B?6Kqe?=')
    expect(result).toBe('日本 語')
  })
})
