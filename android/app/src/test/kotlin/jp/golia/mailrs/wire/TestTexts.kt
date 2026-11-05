package jp.golia.mailrs.wire

import jp.golia.mailrs.R
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

/**
 * The app's words for a JVM test, which has no Android resources: the
 * same `strings.xml` the app reads, looked up by the generated id.
 */
object TestTexts {
    /** The module directory's resources, as the unit test task runs from there. */
    private val res = File("src/main/res")

    /** `name -> text` of one `values*` folder, unescaped as Android does. */
    fun table(folder: String): Map<String, String> {
        val doc = DocumentBuilderFactory.newInstance().newDocumentBuilder()
            .parse(File(res, "$folder/strings.xml"))
        val nodes = doc.getElementsByTagName("string")
        return (0 until nodes.length).associate { i ->
            val n = nodes.item(i)
            n.attributes.getNamedItem("name").nodeValue to unescape(n.textContent)
        }
    }

    /** Use [folder]'s words for every [Texts.get] in this JVM. */
    fun install(folder: String = "values") {
        Texts.install(lookup(folder))
    }

    /**
     * Use [folder]'s words and return the lookup that was in place, for a
     * test that changes the language to hand back to `Texts.swap` after:
     * every test class in the JVM shares the one lookup.
     */
    fun swap(folder: String): Texts.Lookup? = Texts.swap(lookup(folder))

    private fun lookup(folder: String): Texts.Lookup {
        val words = table(folder)
        val names = R.string::class.java.fields.associate { it.getInt(null) to it.name }
        return Texts.Lookup { id, args ->
            val name = checkNotNull(names[id]) { "no string with id $id" }
            val text = checkNotNull(words[name]) { "$folder has no $name" }
            String.format(text, *args)
        }
    }

    private fun unescape(raw: String): String {
        // a value wrapped in plain quotes keeps its spaces at the ends
        val s = when (raw.length > 1 && raw.startsWith("\"") && raw.endsWith("\"")) {
            true -> raw.substring(1, raw.length - 1)
            false -> raw.trim()
        }
        return s.replace("\\'", "'").replace("\\\"", "\"").replace("\\n", "\n").replace("\\@", "@").replace("\\?", "?")
            .replace("\\\\", "\\")
    }
}
