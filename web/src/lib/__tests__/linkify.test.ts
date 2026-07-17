import { describe, expect, it } from 'vitest'

import { splitUrls } from '@/lib/linkify'

describe('splitUrls', () => {
  it('returns a single text segment when there is no url', () => {
    expect(splitUrls('hello world')).toEqual([{ type: 'text', value: 'hello world' }])
  })

  it('detects a bare https url mid-sentence', () => {
    expect(splitUrls('see https://example.com now')).toEqual([
      { type: 'text', value: 'see ' },
      { type: 'url', value: 'https://example.com' },
      { type: 'text', value: ' now' },
    ])
  })

  it('keeps a trailing slash as part of the url', () => {
    const segs = splitUrls('https://www.netbk.co.jp/contents/hojin/gaika/soukin/guide/')
    expect(segs).toEqual([
      { type: 'url', value: 'https://www.netbk.co.jp/contents/hojin/gaika/soukin/guide/' },
    ])
  })

  it('trims trailing ascii sentence punctuation out of the url', () => {
    expect(splitUrls('go to https://example.com/page.')).toEqual([
      { type: 'text', value: 'go to ' },
      { type: 'url', value: 'https://example.com/page' },
      { type: 'text', value: '.' },
    ])
  })

  it('trims trailing CJK punctuation (Japanese period)', () => {
    expect(splitUrls('詳細はhttps://example.com/flow/。')).toEqual([
      { type: 'text', value: '詳細は' },
      { type: 'url', value: 'https://example.com/flow/' },
      { type: 'text', value: '。' },
    ])
  })

  it('handles multiple urls on separate lines', () => {
    const body = 'a https://one.example/ b\nc https://two.example/x d'
    expect(splitUrls(body)).toEqual([
      { type: 'text', value: 'a ' },
      { type: 'url', value: 'https://one.example/' },
      { type: 'text', value: ' b\nc ' },
      { type: 'url', value: 'https://two.example/x' },
      { type: 'text', value: ' d' },
    ])
  })

  it('ignores non-http schemes', () => {
    expect(splitUrls('run javascript:alert(1) please')).toEqual([
      { type: 'text', value: 'run javascript:alert(1) please' },
    ])
  })
})
