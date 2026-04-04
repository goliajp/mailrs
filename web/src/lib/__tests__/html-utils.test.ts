import { describe, expect, it } from 'vitest'

import {
  buildForwardHeader,
  buildForwardHeaderHtml,
  escapeHtml,
  formatFileSize,
} from '../html-utils'

describe('escapeHtml', () => {
  it('returns plain text unchanged', () => {
    expect(escapeHtml('hello world')).toBe('hello world')
  })

  it('escapes ampersand', () => {
    expect(escapeHtml('a & b')).toBe('a &amp; b')
  })

  it('escapes angle brackets', () => {
    expect(escapeHtml('<script>')).toBe('&lt;script&gt;')
  })

  it('escapes double quotes', () => {
    expect(escapeHtml('say "hello"')).toBe('say &quot;hello&quot;')
  })

  it('escapes mixed special characters', () => {
    expect(escapeHtml('<a href="x">&</a>')).toBe('&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;')
  })

  it('returns empty string unchanged', () => {
    expect(escapeHtml('')).toBe('')
  })
})

describe('formatFileSize', () => {
  it('returns 0B for zero bytes', () => {
    expect(formatFileSize(0)).toBe('0B')
  })

  it('returns bytes for values under 1024', () => {
    expect(formatFileSize(512)).toBe('512B')
    expect(formatFileSize(1023)).toBe('1023B')
  })

  it('returns KB for 1024', () => {
    expect(formatFileSize(1024)).toBe('1KB')
  })

  it('returns KB for values in KB range', () => {
    expect(formatFileSize(5120)).toBe('5KB')
    expect(formatFileSize(102400)).toBe('100KB')
  })

  it('returns KB for value just below MB boundary', () => {
    expect(formatFileSize(1048575)).toBe('1024KB')
  })

  it('returns MB for 1048576', () => {
    expect(formatFileSize(1048576)).toBe('1.0MB')
  })

  it('returns MB for values in MB range', () => {
    expect(formatFileSize(5 * 1024 * 1024)).toBe('5.0MB')
    expect(formatFileSize(15.7 * 1024 * 1024)).toBe('15.7MB')
  })
})

describe('buildForwardHeader', () => {
  it('formats forward header with all fields', () => {
    const result = buildForwardHeader('alice@example.com', '2024-01-15', 'Test Subject')
    expect(result).toBe(
      '---------- Forwarded message ----------\n' +
        'From: alice@example.com\n' +
        'Date: 2024-01-15\n' +
        'Subject: Test Subject\n'
    )
  })

  it('includes all three fields in output', () => {
    const result = buildForwardHeader('sender', 'date-val', 'subj-val')
    expect(result).toContain('From: sender')
    expect(result).toContain('Date: date-val')
    expect(result).toContain('Subject: subj-val')
  })
})

describe('buildForwardHeaderHtml', () => {
  it('formats forward header as HTML', () => {
    const result = buildForwardHeaderHtml('bob@example.com', '2024-03-01', 'Hello')
    expect(result).toContain('---------- Forwarded message ----------')
    expect(result).toContain('From: bob@example.com')
    expect(result).toContain('Date: 2024-03-01')
    expect(result).toContain('Subject: Hello')
    expect(result).toMatch(/^<p.*>.*<\/p>$/)
  })

  it('escapes special characters in inputs', () => {
    const result = buildForwardHeaderHtml(
      '<script>alert("xss")</script>',
      '2024 & beyond',
      'Re: "important"'
    )
    expect(result).toContain('&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;')
    expect(result).toContain('2024 &amp; beyond')
    expect(result).toContain('Re: &quot;important&quot;')
    expect(result).not.toContain('<script>')
  })
})
