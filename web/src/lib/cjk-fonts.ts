// Which CJK fonts a mail body is set in.
//
// A mail's own font-family names Latin fonts (Roboto, Google Sans, Arial),
// so every CJK character falls back. With one Japanese list for all mail,
// characters a Japanese font lacks — simplified forms such as 传 or 宣 in
// some faces — fall back again, glyph by glyph, to whatever other font has
// them, and a heading comes out in two weights and two designs. The list
// has to be the mail's own script, and the element has to carry the
// language so the browser picks the regional glyph forms.

export type CjkLang = 'ja' | 'ko' | 'zh-Hans' | 'zh-Hant'

const FONTS: Record<CjkLang, string> = {
  ja: "'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Yu Gothic', 'Meiryo', 'Noto Sans CJK JP', 'Noto Sans JP'",
  ko: "'Apple SD Gothic Neo', 'Malgun Gothic', 'Noto Sans CJK KR', 'Noto Sans KR'",
  'zh-Hans':
    "'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', 'Noto Sans CJK SC', 'Noto Sans SC'",
  'zh-Hant': "'PingFang TC', 'Microsoft JhengHei', 'Noto Sans CJK TC', 'Noto Sans TC'",
}

const EMOJI = "'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji'"

// common characters that differ between the two written forms
const SIMPLIFIED = new Set(
  '这们说时会对国个来为后发经过还动问东车开关见长现体实点电话书学门马让给们气样从认题应头将'
)
const TRADITIONAL = new Set(
  '這們說時會對國個來為後發經過還動問東車開關見長現體實點電話書學門馬讓給們氣樣從認題應頭將'
)

const KANA = /[぀-ヿ]/
const HANGUL = /[가-힯]/
const HAN = /[一-鿿]/

/** The CJK fonts, then emoji, for a body in `lang`; Japanese when it has none. */
export function cjkFonts(lang: CjkLang | null): string {
  return `${FONTS[lang ?? 'ja']}, ${EMOJI}`
}

/** The CJK language a body is written in, or `null` when it has no CJK text. */
export function cjkLang(text: string): CjkLang | null {
  if (KANA.test(text)) return 'ja'
  if (HANGUL.test(text)) return 'ko'
  if (!HAN.test(text)) return null
  let simplified = 0
  let traditional = 0
  for (const ch of text) {
    if (SIMPLIFIED.has(ch)) simplified += 1
    if (TRADITIONAL.has(ch)) traditional += 1
  }
  if (traditional > simplified) return 'zh-Hant'
  return 'zh-Hans'
}

/**
 * Append the CJK fonts to every font-family the mail declares, so its
 * CJK text is set in one family rather than fall back glyph by glyph.
 */
export function injectCjkFonts(html: string, lang: CjkLang | null): string {
  const fonts = cjkFonts(lang)
  return html.replace(/font-family\s*:\s*([^;}"]+)/gi, (match, declared: string) => {
    if (declared.includes(fonts.slice(0, 12))) return match
    const trimmed = declared.trimEnd()
    let base = trimmed
    if (trimmed.endsWith(',')) base = trimmed.slice(0, -1)
    return `font-family: ${base}, ${fonts}`
  })
}
