package jp.golia.mailrs

import android.content.res.Configuration
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.Locale
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The string resources against `wording.json`, the wording table the web
 * reads and the app ships a byte-identical copy of
 * (`scripts/check-wording.sh` compares the copies and the XML). This reads
 * what the installed app resolves in each language, on a device.
 */
@RunWith(AndroidJUnit4::class)
class WordingTableTest {
    private val app = InstrumentationRegistry.getInstrumentation().targetContext

    /** The table's language code and the locale Android picks the folder by. */
    private val languages = listOf("en" to Locale.ENGLISH, "ja" to Locale.JAPANESE, "zh" to Locale.SIMPLIFIED_CHINESE)

    @Test
    fun every_key_says_what_the_table_says() {
        val terms = Json.parseToJsonElement(app.assets.open("wording.json").bufferedReader().readText())
            .jsonObject.getValue("terms").jsonArray.map { it.jsonObject }
        assertTrue("the table is empty", terms.isNotEmpty())
        for ((lang, locale) in languages) {
            val config = Configuration(app.resources.configuration).apply { setLocale(locale) }
            val resources = app.createConfigurationContext(config).resources
            for (term in terms) {
                var slot = 0
                // the table writes the web's `{name}`; the resources, `%1$s`
                val want = term.getValue("text").jsonObject.getValue(lang).jsonPrimitive.content
                    .replace(Regex("""\{\w+\}""")) { slot += 1; "%$slot\$s" }
                for (name in term.getValue("android").jsonArray.map { it.jsonPrimitive.content }) {
                    val id = R.string::class.java.getField(name).getInt(null)
                    assertEquals("${term.getValue("id")}: $name in $lang", want, resources.getString(id))
                }
            }
        }
    }
}
