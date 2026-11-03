package jp.golia.mailrs.wire

import android.content.Context
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json

/**
 * Why a message was judged as it was, in the reader's language.
 *
 * The server stores a reason as a code and the values its sentence
 * names; the words come from `assets/reasons.json`, a byte-identical copy
 * of the server's table (a Rust test compares every copy). The rule is
 * the web client's: the sentence that names the facts when the reason
 * carries every one of them, the general sentence otherwise.
 */
class ReasonText(private val table: Map<String, Entry>) {

    /** The table's three columns. */
    enum class Lang {
        En, Ja, Zh;

        companion object {
            /** Japanese and Chinese have their own column, everything else reads English. */
            fun of(language: String): Lang = when (language) {
                "ja" -> Ja
                "zh" -> Zh
                else -> En
            }
        }
    }

    @Serializable
    data class Sentences(val en: String, val ja: String, val zh: String) {
        fun of(lang: Lang): String = when (lang) {
            Lang.En -> en
            Lang.Ja -> ja
            Lang.Zh -> zh
        }
    }

    @Serializable
    data class Entry(val en: String, val ja: String, val zh: String, val params: Sentences? = null) {
        val general get() = Sentences(en, ja, zh)
    }

    val size: Int get() = table.size
    val entries: Map<String, Entry> get() = table

    fun has(code: String): Boolean = table.containsKey(code)

    /**
     * The sentence for [reason], or null when this build has none for its
     * code: a rule added on the server after this build.
     */
    fun text(reason: WireVerdict.Reason, lang: Lang): String? {
        val entry = table[reason.code] ?: return null
        // one pass, so a value that itself reads `{host}` stays as written
        return PLACEHOLDER.replace(sentence(entry, lang, reason.params)) { m ->
            val name = m.groupValues[1]
            val value = reason.params[name] ?: return@replace m.value
            if (name == "class") return@replace CLASS_NAMES[value]?.of(lang) ?: value
            value
        }
    }

    /** The sentence, or the code itself when there is none. */
    fun words(reason: WireVerdict.Reason, lang: Lang): String = text(reason, lang) ?: reason.code

    private fun sentence(entry: Entry, lang: Lang, params: Map<String, String>): String {
        val withParams = entry.params?.of(lang) ?: return entry.general.of(lang)
        val names = PLACEHOLDER.findAll(withParams).map { it.groupValues[1] }
        if (names.all { params.containsKey(it) }) return withParams
        return entry.general.of(lang)
    }

    companion object {
        private val PLACEHOLDER = Regex("""\{(\w+)\}""")

        /** The category names a `class` param can carry, as the reader knows them. */
        private val CLASS_NAMES = mapOf(
            "inbox" to Sentences("Inbox", "受信トレイ", "收件箱"),
            "notification" to Sentences("Notifications", "通知", "通知"),
            "promotion" to Sentences("Promotions", "プロモーション", "推广"),
        )

        private val json = Json { ignoreUnknownKeys = true }

        fun parse(text: String): ReasonText =
            ReasonText(json.decodeFromString(MapSerializer(String.serializer(), Entry.serializer()), text))

        @Volatile
        private var loaded: ReasonText? = null

        /** The table shipped in the app, read once. */
        fun bundled(context: Context): ReasonText = loaded ?: synchronized(this) {
            loaded ?: parse(
                context.applicationContext.assets.open("reasons.json").bufferedReader().use { it.readText() },
            ).also { loaded = it }
        }
    }
}
