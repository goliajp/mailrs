import { describe, expect, it } from 'vitest'

import { avatarColor, avatarInitial, extractEmail, extractName } from '../avatar'

describe('avatarColor edge cases', () => {
  it('handles very long email addresses', () => {
    const longEmail = 'a'.repeat(1000) + '@example.com'
    const result = avatarColor(longEmail)
    expect(typeof result).toBe('string')
    expect(result).toMatch(/^bg-\w+-500$/)
  })

  it('handles email with special characters', () => {
    const result = avatarColor('user+tag@example.com')
    expect(typeof result).toBe('string')
    expect(result).toMatch(/^bg-\w+-500$/)
  })

  it('handles email with dots', () => {
    const result = avatarColor('first.last@sub.domain.co.uk')
    expect(typeof result).toBe('string')
    expect(result).toMatch(/^bg-\w+-500$/)
  })

  it('returns same result across multiple calls', () => {
    const email = 'consistent@example.com'
    const results = Array.from({ length: 10 }, () => avatarColor(email))
    expect(new Set(results).size).toBe(1)
  })
})

describe('avatarInitial edge cases', () => {
  it('handles email-only with numeric start', () => {
    expect(avatarInitial('123user@example.com')).toBe('1')
  })

  it('handles display name with special chars', () => {
    expect(avatarInitial('O\'Brien <obrien@example.com>')).toBe('O')
  })

  it('handles display name with unicode', () => {
    const result = avatarInitial('Tanaka <tanaka@example.jp>')
    expect(result).toBe('T')
  })

  it('handles single character name', () => {
    expect(avatarInitial('A <a@example.com>')).toBe('A')
  })

  it('handles only angle brackets', () => {
    expect(avatarInitial('<a@example.com>')).toBe('<')
  })
})

describe('extractEmail edge cases', () => {
  it('handles nested angle brackets', () => {
    expect(extractEmail('Name <<user@example.com>>')).toBe('<user@example.com')
  })

  it('handles email with plus addressing', () => {
    expect(extractEmail('User <user+tag@example.com>')).toBe('user+tag@example.com')
  })

  it('returns empty string as-is', () => {
    expect(extractEmail('')).toBe('')
  })

  it('handles IP address domain', () => {
    expect(extractEmail('User <user@[192.168.1.1]>')).toBe('user@[192.168.1.1]')
  })

  it('handles multiple @ signs in display name', () => {
    expect(extractEmail('"user@work" <user@personal.com>')).toBe('user@personal.com')
  })
})

describe('extractName edge cases', () => {
  it('handles email with no local part', () => {
    expect(extractName('@example.com')).toBe('')
  })

  it('handles display name with numbers', () => {
    expect(extractName('User123 <user123@example.com>')).toBe('User123')
  })

  it('handles very long display name', () => {
    const longName = 'A'.repeat(200)
    const result = extractName(`${longName} <a@example.com>`)
    expect(result).toBe(longName)
  })

  it('handles display name with commas', () => {
    expect(extractName('"Doe, John" <john@example.com>')).toBe('Doe, John')
  })

  it('handles bare domain (no @ in input)', () => {
    expect(extractName('localhost')).toBe('localhost')
  })

  it('handles empty string', () => {
    expect(extractName('')).toBe('')
  })
})
