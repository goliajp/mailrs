package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The three languages carry the same words: a key in English and not in
 * Japanese falls back to English on a Japanese phone, which is the bug
 * this exists to stop. `scripts/check-android-l10n.sh` checks the same
 * thing in the deploy gate, together with the source scan.
 */
class LocalizationTest {
    private val untranslatable = setOf("app_name", "widget_preview_sender_one", "widget_preview_sender_two")

    @Test
    fun every_language_has_every_key() {
        val en = TestTexts.table("values").keys - untranslatable
        for (folder in listOf("values-ja", "values-zh-rCN")) {
            val other = TestTexts.table(folder).keys
            assertEquals("$folder is missing ${en - other}", emptySet<String>(), en - other)
            assertEquals("$folder has keys English has not: ${other - en}", emptySet<String>(), other - en)
        }
    }

    @Test
    fun a_placeholder_is_the_same_in_every_language() {
        val placeholder = Regex("%\\d\\$[sd]")
        val en = TestTexts.table("values")
        for (folder in listOf("values-ja", "values-zh-rCN")) {
            for ((key, text) in TestTexts.table(folder)) {
                val want = placeholder.findAll(en.getValue(key)).map { it.value }.sorted().toList()
                val got = placeholder.findAll(text).map { it.value }.sorted().toList()
                assertEquals("$folder $key", want, got)
            }
        }
    }

    /** Chinese and Japanese use full-width punctuation, and short UI text ends without a full stop. */
    @Test
    fun cjk_text_is_punctuated_as_cjk() {
        for (folder in listOf("values-ja", "values-zh-rCN")) {
            for ((key, text) in TestTexts.table(folder)) {
                assertTrue("$folder $key ends with a full stop: $text", !text.endsWith("。"))
                assertTrue("$folder $key uses a half-width comma between CJK: $text", !Regex("\\p{IsHan}, ?\\p{IsHan}").containsMatchIn(text))
            }
        }
    }

    @Test
    fun the_japanese_words_are_the_ones_shown() {
        TestTexts.install("values-ja")
        assertEquals("受信トレイ", Texts.get(MailList.Inbox.title))
        assertEquals("ゴミ箱に移動しました", Texts.get(RowAction.Trash.done!!))
        TestTexts.install("values-zh-rCN")
        assertEquals("已删除邮件", Texts.get(MailList.Trash.title))
        TestTexts.install()
    }
}
