package jp.golia.mailrs.wire

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The renderer against the shared samples the web send path produced
 * (`web/src/lib/markdown-samples.json`, copied to this test's resources and
 * kept the same bytes by `scripts/check-markdown-samples.sh`).
 */
class MarkdownHtmlTest {
    @Serializable
    private data class Sample(val name: String, val markdown: String, val html: String)

    @Serializable
    private data class Wrap(val inner: String, val html: String)

    @Serializable
    private data class Samples(val samples: List<Sample>, val wrap: Wrap)

    private val shared: Samples = Json.decodeFromString(
        Samples.serializer(),
        javaClass.getResource("/markdown-samples.json")!!.readText(),
    )

    @Test
    fun `every shared sample renders to the web's bytes`() {
        val wrong = shared.samples.filter { MarkdownHtml.render(it.markdown) != it.html }.map { it.name }
        assertTrue("these render differently from the web: $wrong", wrong.isEmpty())
        assertTrue("there are no samples to check", shared.samples.size >= 40)
    }

    @Test
    fun `the email wrapper is the web's`() {
        assertEquals(shared.wrap.html, MarkdownHtml.wrap(shared.wrap.inner))
    }

    @Test
    fun `only a body with marks of its own gets html`() {
        assertNull(MarkdownHtml.forSend("Thanks, see https://example.com\n\nOn Mon, A wrote:\n> q"))
        val html = MarkdownHtml.forSend("Hello **big**")
        assertTrue(html.orEmpty().contains("<p>Hello <strong>big</strong></p>"))
        assertTrue(html.orEmpty().startsWith("<!DOCTYPE html>"))
        assertFalse(MarkdownHtml.forSend("x [y](javascript:alert(1))").orEmpty().contains("javascript"))
    }
}
