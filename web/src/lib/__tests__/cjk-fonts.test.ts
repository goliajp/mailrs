import { describe, expect, it } from 'vitest'

import { cjkFonts, cjkLang, injectCjkFonts } from '../cjk-fonts'

describe('cjkLang', () => {
  it('reads the script a body is written in', () => {
    expect(cjkLang('在 Google 上宣传您的折扣和优惠')).toBe('zh-Hans')
    expect(cjkLang('在 Google 上宣傳您的折扣和優惠，這個')).toBe('zh-Hant')
    expect(cjkLang('楽天銀行カード（JCB）をご利用いただき')).toBe('ja')
    expect(cjkLang('결제가 완료되었습니다')).toBe('ko')
    expect(cjkLang('Review of your submission is complete.')).toBeNull()
  })
})

describe('injectCjkFonts', () => {
  it('appends the fonts of the body language to each declared family', () => {
    const out = injectCjkFonts('<p style="font-family: Roboto, Arial;">x</p>', 'zh-Hans')
    expect(out).toContain("font-family: Roboto, Arial, 'PingFang SC'")
    expect(out).not.toContain('Hiragino Sans,')
  })

  it('does not append twice', () => {
    const once = injectCjkFonts('<p style="font-family: Roboto">x</p>', 'ja')
    expect(injectCjkFonts(once, 'ja')).toBe(once)
  })

  it('falls back to the Japanese list for a body with no CJK text', () => {
    expect(cjkFonts(null).startsWith("'Hiragino Sans'")).toBe(true)
  })
})
