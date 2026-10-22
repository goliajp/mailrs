package jp.golia.mailrs.wire

/**
 * The CJK language a mail body is written in, as a `lang` tag.
 *
 * A mail names Latin fonts the phone does not have, so every CJK character
 * falls back, and without a language the web view falls back by the phone's
 * locale: a Chinese mail on a Japanese phone is set partly in the Japanese
 * font and partly in another, in two weights. Naming the language picks the
 * regional font for every glyph. Same rule as the web client's
 * `cjk-fonts.ts` and iOS `CjkLang`.
 */
object CjkLang {
    private val SIMPLIFIED = "这们说时会对国个来为后发经过还动问东车开关见长现体实点电话书学门马让给气样从认题应头将".toSet()
    private val TRADITIONAL = "這們說時會對國個來為後發經過還動問東車開關見長現體實點電話書學門馬讓給氣樣從認題應頭將".toSet()

    /** `ja`, `ko`, `zh-Hans`, `zh-Hant`, or `null` when the body has no CJK text. */
    fun of(text: String): String? {
        var han = false
        for (c in text) {
            when (c) {
                in '぀'..'ヿ' -> return "ja"
                in '가'..'힯' -> return "ko"
                in '一'..'鿿' -> han = true
                else -> {}
            }
        }
        if (!han) return null
        val simplified = text.count { it in SIMPLIFIED }
        val traditional = text.count { it in TRADITIONAL }
        if (traditional > simplified) return "zh-Hant"
        return "zh-Hans"
    }
}
