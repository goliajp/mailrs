import { describe, expect, it } from 'vitest'

import { applyFormat, formatForKey } from '../markdown-format'

describe('applyFormat', () => {
  it('wraps the selection in bold and keeps it selected', () => {
    const e = applyFormat('say hello now', 4, 9, 'bold')
    expect(e.value).toBe('say **hello** now')
    expect(e.value.slice(e.start, e.end)).toBe('hello')
  })

  it('takes the marks off a selection that already has them', () => {
    const e = applyFormat('say **hello** now', 6, 11, 'bold')
    expect(e.value).toBe('say hello now')
    expect(e.value.slice(e.start, e.end)).toBe('hello')
  })

  it('wraps an empty selection, leaving the caret between the marks', () => {
    const e = applyFormat('ab', 1, 1, 'italic')
    expect(e.value).toBe('a__b')
    expect(e.start).toBe(2)
    expect(e.end).toBe(2)
  })

  it('makes a link of the selection with the url placeholder selected', () => {
    const e = applyFormat('see docs', 4, 8, 'link')
    expect(e.value).toBe('see [docs](https://)')
    expect(e.value.slice(e.start, e.end)).toBe('https://')
  })

  it('prefixes every line the selection touches', () => {
    const text = 'intro\none\ntwo\nend'
    const e = applyFormat(text, 7, 11, 'list')
    expect(e.value).toBe('intro\n- one\n- two\nend')
    const n = applyFormat(text, 7, 11, 'numbered')
    expect(n.value).toBe('intro\n1. one\n2. two\nend')
    const q = applyFormat(text, 0, 0, 'quote')
    expect(q.value).toBe('> intro\none\ntwo\nend')
  })

  it('reads the keyboard shortcuts', () => {
    expect(formatForKey('b')).toBe('bold')
    expect(formatForKey('I')).toBe('italic')
    expect(formatForKey('k')).toBe('link')
    expect(formatForKey('x')).toBeNull()
  })
})
