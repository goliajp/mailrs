import { describe, expect, it } from 'vitest'

import { avatarColor, avatarInitial, extractEmail, extractName } from '../avatar'

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
    const result = extractName('<0101019ccdad21d4-3b587183-7366-4b5f-a157-c01ce00c45e1-000000@atlassian-bounces.atlassian.net>')
    expect(result).toBe('Atlassian')
  })

  it('returns domain-based name for long hex local parts', () => {
    const result = extractName('01234567890abcdef01234567@mailer.example.com')
    expect(result).toBe('Mailer')
  })

  it('keeps short normal local parts as-is', () => {
    expect(extractName('noreply@github.com')).toBe('noreply')
  })
})
