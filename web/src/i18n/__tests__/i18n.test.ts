// The three dictionaries say the same things: the same keys, nothing
// empty, and the same placeholders, so a translation cannot drop the
// sender's name out of a sentence.

import { describe, expect, it } from 'vitest'

import { browserLang, translate } from '@/i18n'
import { en } from '@/i18n/en'
import { ja } from '@/i18n/ja'
import { zh } from '@/i18n/zh'

const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()

describe('dictionaries', () => {
  for (const [name, dict] of Object.entries({ ja, zh })) {
    it(`${name} has every key, none empty, each with the same placeholders`, () => {
      expect(Object.keys(dict).sort()).toEqual(Object.keys(en).sort())
      for (const [key, text] of Object.entries(dict)) {
        expect(text.trim(), key).not.toBe('')
        expect(placeholders(text), key).toEqual(placeholders(en[key as keyof typeof en]))
      }
    })
  }
})

describe('translate', () => {
  it('fills placeholders and leaves an unknown one as written', () => {
    expect(translate('en', 'live.newMail', { sender: 'Ann', subject: 'lunch' })).toBe(
      'New mail from Ann: lunch'
    )
    expect(translate('zh', 'live.newMail', { sender: 'Ann' })).toBe('新邮件，来自 Ann：{subject}')
    expect(translate('ja', 'list.inbox')).toBe('受信トレイ')
  })

  it('takes the browser language as one of ours', () => {
    expect(browserLang('zh-CN')).toBe('zh')
    expect(browserLang('zh-Hant-TW')).toBe('zh')
    expect(browserLang('ja-JP')).toBe('ja')
    expect(browserLang('en-GB')).toBe('en')
    expect(browserLang('fr')).toBe('en')
  })
})
