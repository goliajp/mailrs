import { describe, expect, it } from 'vitest'

import { translate } from '@/i18n'
import { reasonText } from '@/lib/reasons'
import table from '@/lib/reasons.json'

const tFor = (lang: 'en' | 'ja' | 'zh') => (key: Parameters<typeof translate>[1]) =>
  translate(lang, key)

describe('reason sentences', () => {
  it('fill in the values a reason carries, in the reader’s language', () => {
    const r = { code: 'junk-score', params: { score: '6.5', threshold: '5.0' } }
    expect(reasonText(r, 'en', tFor('en'))).toBe(
      'The checks added up to 6.5, at or above the Junk threshold of 5.0.'
    )
    expect(reasonText(r, 'zh', tFor('zh'))).toBe('各项检查合计 6.5 分，达到垃圾邮件阈值 5.0。')
  })

  it('name a category the way the reader knows it', () => {
    const r = { code: 'sorted-by-classifier', params: { class: 'promotion' } }
    expect(reasonText(r, 'ja', tFor('ja'))).toContain('「プロモーション」')
  })

  it('answer null for a code this build has no sentence for', () => {
    expect(reasonText({ code: 'a-rule-added-later' }, 'en', tFor('en'))).toBeNull()
  })

  it('exist in every language for every code', () => {
    for (const [code, langs] of Object.entries(table)) {
      for (const lang of ['en', 'ja', 'zh'] as const) {
        expect(langs[lang], `${code} ${lang}`).toBeTruthy()
      }
    }
  })
})
