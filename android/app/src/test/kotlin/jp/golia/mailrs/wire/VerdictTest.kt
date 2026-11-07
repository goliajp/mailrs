package jp.golia.mailrs.wire

import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The reasons table the app ships, read the way the app reads it. */
private val table: ReasonText by lazy {
    ReasonText.parse(File("src/main/assets/reasons.json").readText())
}

class ReasonTextTest {
    /** The copy is the server's table (a Rust test compares the bytes); this reads it. */
    @Test
    fun `the shipped table has every code in three languages`() {
        assertTrue(table.size > 30)
        for ((code, e) in table.entries) {
            assertTrue(code, e.en.isNotEmpty() && e.ja.isNotEmpty() && e.zh.isNotEmpty())
            e.params?.let { assertTrue("$code params", it.en.isNotEmpty() && it.ja.isNotEmpty() && it.zh.isNotEmpty()) }
        }
    }

    @Test
    fun `a reason carrying every fact gets the sentence that names them`() {
        val r = WireVerdict.Reason("claims-our-name", mapOf("name" to "Golia"))
        assertEquals("The display name claims to be Golia.", table.text(r, ReasonText.Lang.En))
        assertEquals("表示名が「Golia」を名乗っています。", table.text(r, ReasonText.Lang.Ja))
        assertEquals("显示名自称是「Golia」。", table.text(r, ReasonText.Lang.Zh))
    }

    @Test
    fun `a reason stored before the rule named its facts gets the general sentence`() {
        val r = WireVerdict.Reason("bec-pay-to-a-stranger", mapOf("domain" to "x.example"))
        assertEquals("The first message from this address carries a payment account.", table.text(r, ReasonText.Lang.En))
    }

    @Test
    fun `a category is named the way the reader knows it`() {
        val r = WireVerdict.Reason("sorted-by-classifier", mapOf("class" to "promotion"))
        assertTrue(table.text(r, ReasonText.Lang.En)!!.contains("Promotions"))
        assertTrue(table.text(r, ReasonText.Lang.Ja)!!.contains("プロモーション"))
        assertTrue(table.text(r, ReasonText.Lang.Zh)!!.contains("推广"))
    }

    @Test
    fun `each check says what it said, and an unknown code falls back to the code`() {
        assertEquals("DKIM: fail", table.text(WireVerdict.Reason("auth-check", mapOf("check" to "DKIM", "result" to "fail")), ReasonText.Lang.En))
        assertNull(table.text(WireVerdict.Reason("rule-added-later"), ReasonText.Lang.En))
        assertEquals("rule-added-later", table.words(WireVerdict.Reason("rule-added-later"), ReasonText.Lang.En))
    }

    @Test
    fun `a value that reads like a placeholder is shown as written`() {
        val r = WireVerdict.Reason("link-shows-another-host", mapOf("shown" to "{host}", "host" to "a.example"))
        assertEquals("This link shows {host} but goes to a.example.", table.text(r, ReasonText.Lang.En))
    }

    @Test
    fun `the column follows the interface language`() {
        assertEquals(ReasonText.Lang.Ja, ReasonText.Lang.of("ja"))
        assertEquals(ReasonText.Lang.Zh, ReasonText.Lang.of("zh"))
        assertEquals(ReasonText.Lang.En, ReasonText.Lang.of("fr"))
    }
}

class FraudVerdictWireTest {
    private val json = Json { ignoreUnknownKeys = true; explicitNulls = false }

    @Test
    fun `a stored verdict decodes`() {
        val v = json.decodeFromString(
            WireVerdict.Response.serializer(),
            """{"verdict":{"layers":[{"name":"identity","outcome":"fail","detail":"d",
              "reasons":[{"code":"claims-our-name","params":{"name":"Golia"}}],"score":4.0},
              {"name":"transport","outcome":"pass","detail":"","score":0}],
              "placement":[{"code":"junk-your-filter"}],"quarantined":true,
              "rules_version":"2026-11-01.3","score":4.0}}""",
        ).verdict!!
        assertTrue(v.quarantined)
        assertEquals("2026-11-01.3", v.rulesVersion)
        assertEquals(listOf("identity", "transport"), v.layers.map { it.name })
        assertEquals(mapOf("name" to "Golia"), v.layers[0].reasons[0].params)
        assertTrue(v.layers[1].reasons.isEmpty())
        assertEquals(listOf(WireVerdict.Reason("junk-your-filter")), v.placement)
    }

    /**
     * A content layer as `mailrs_inbound::assess` stores it: the content scan
     * as one line, each content rule as its own, each with what it added. The
     * numbers the panel writes under the layer add up to the one beside it.
     */
    @Test
    fun `every content rule is a line, and the layer is their sum`() {
        val v = json.decodeFromString(
            WireVerdict.Response.serializer(),
            """{"verdict":{"layers":[{"name":"transport","outcome":"pass","detail":"",
              "reasons":[{"code":"auth-check","params":{"check":"SPF","result":"pass"}}],"score":0},
              {"name":"content","outcome":"fail","detail":"d","score":12.5,
              "reasons":[{"code":"content-score","params":{"content":"1.5","classifier":"0.0","rules":""},"score":1.5},
              {"code":"offers-the-reader-a-sum","score":5},{"code":"asks-for-the-readers-line","score":6}]}],
              "quarantined":true,"rules_version":"2026-11-06.1","score":12.5}}""",
        ).verdict!!
        val content = v.layers.first { it.name == "content" }
        assertEquals(
            listOf("content-score", "offers-the-reader-a-sum", "asks-for-the-readers-line"),
            content.reasons.map { it.code },
        )
        val shown = content.reasons.mapNotNull { it.score }.map { jp.golia.mailrs.ui.scoreLabel(it).drop(1).toDouble() }
        assertEquals(content.reasons.size, shown.size)
        assertEquals(jp.golia.mailrs.ui.scoreLabel(content.score), jp.golia.mailrs.ui.scoreLabel(shown.sum()))
        // the authentication checks add nothing and carry no number
        assertTrue(v.layers[0].reasons.all { it.score == null })
    }

    @Test
    fun `no verdict is null, not an error`() {
        assertNull(json.decodeFromString(WireVerdict.Response.serializer(), """{"verdict":null}""").verdict)
    }
}

class LinkWarningsTest {
    private val shows = WireVerdict.LinkWarning(
        "webmail-oauth.wzldq.com", "www.golia.jp", listOf(WireVerdict.Reason("link-shows-another-host")),
    )
    private val listed = WireVerdict.LinkWarning("bad.example", null, listOf(WireVerdict.Reason("link-host-listed")))

    @Test
    fun `an anchor showing another host is matched by its text`() {
        assertEquals(shows, LinkWarnings.warning("https://webmail-oauth.wzldq.com/auth", "www.golia.jp/ lihao", listOf(shows)))
        assertNull(LinkWarnings.warning("https://webmail-oauth.wzldq.com/auth", "sign in", listOf(shows)))
    }

    @Test
    fun `a listed host is matched whatever the text`() {
        assertEquals(listed, LinkWarnings.warning("https://BAD.example/x", "here", listOf(listed)))
        assertNull(LinkWarnings.warning("https://fine.example/", "here", listOf(listed)))
        assertNull(LinkWarnings.warning("not a url at all", "here", listOf(listed)))
    }

    @Test
    fun `a flagged anchor is pointed at the question, with a mark after it`() {
        val html = """<p><a href="https://webmail-oauth.wzldq.com/auth?a=1&amp;b=2">www.golia.jp</a>""" +
            """ and <a href='https://fine.example/'>fine</a></p>"""
        val marked = LinkWarnings.mark(html, listOf(shows, listed)) { "why \"so\"" }
        assertEquals(1, marked.targets.size)
        assertEquals("https://webmail-oauth.wzldq.com/auth?a=1&b=2", marked.targets[0].url)
        assertEquals(shows, marked.targets[0].warning)
        assertFalse(marked.html.contains("wzldq"))
        assertTrue(marked.html.contains("""<a href="mailrs-link-warning:0">www.golia.jp</a>"""))
        assertTrue(marked.html.contains("""aria-label="why &quot;so&quot;""""))
        assertTrue(marked.html.contains("""<a href='https://fine.example/'>fine</a>"""))
        assertEquals(0, LinkWarnings.index("mailrs-link-warning:0"))
        assertNull(LinkWarnings.index("https://fine.example/"))
    }

    @Test
    fun `with no warnings the body is untouched`() {
        val html = """<a href="https://bad.example/">x</a>"""
        assertEquals(html, LinkWarnings.mark(html, emptyList()) { "" }.html)
        assertEquals(html, LinkWarnings.mark(html, listOf(shows)) { "" }.html)
    }
}

class CallbackNumbersTest {
    private fun message(uid: Int, sender: String, text: String?, html: String? = null) = Wire.Message(
        uid = uid, sender = sender, senderTrust = "", recipients = "me", subject = "s", flags = 0,
        internalDate = 1, messageId = "<$uid>", textBody = text, htmlBody = html, category = "inbox",
        riskScore = 0, riskReason = "",
    )

    /** `callback-numbers.json`, the web's table, as the app ships it. */
    private val shared = Json.parseToJsonElement(File("src/main/resources/callback-numbers.json").readText()).jsonObject

    @Test
    fun `every shared sample reads as it does on the web`() {
        val samples = shared.getValue("samples").jsonArray.map { it.jsonObject }
        assertTrue("there are no samples to check", samples.size >= 10)
        assertEquals(shared.getValue("labels").jsonArray.map { it.jsonPrimitive.content }, CallbackNumbers.labels)
        val wrong = samples.mapNotNull { s ->
            val got = CallbackNumbers.phoneNumbers(s.getValue("text").jsonPrimitive.content)
            val want = s.getValue("numbers").jsonArray.map { it.jsonPrimitive.content }
            if (got == want) null else "${s.getValue("name").jsonPrimitive.content}: $got"
        }
        assertEquals(emptyList<String>(), wrong)
    }

    @Test
    fun `a purchase order number is not joined to the invoice's year after it`() {
        val html = "<table><tr><td>Invoice for Phase 1: Under PO 4300078149</td>\n<td>2026-INV-003001-2</td></tr></table>"
        assertEquals(emptyList<String>(), CallbackNumbers.new(message(1, "billing@vendor.example", null, html), emptyList()))
    }

    @Test
    fun `grouped numbers are found, account numbers and dates are not`() {
        val text = "Call +81 3-1234-5678 or (03) 1234 5678. Order 12345678901. On 2026-11-01 12.30.45"
        assertEquals(listOf("+81 3-1234-5678", "(03) 1234 5678"), CallbackNumbers.phoneNumbers(text))
    }

    @Test
    fun `a number the same sender gave before is not pointed out`() {
        val first = message(1, "Bank <a@bank.example>", "Desk: 03-1234-5678")
        val later = message(2, "bank <A@bank.example>", "Call 03-1234-5678 or 0120-111-222")
        assertEquals(listOf("0120-111-222"), CallbackNumbers.new(later, listOf(first)))
        val other = message(3, "x@other.example", "Call 03-1234-5678")
        assertEquals(listOf("03-1234-5678"), CallbackNumbers.new(other, listOf(first)))
    }

    @Test
    fun `the sender history decodes as the server sends it`() {
        val fixture = File("../../wire-contract/responses/callback-numbers.json").readText()
        val r = Json.decodeFromString(WireCallbackNumbers.Response.serializer(), fixture)
        assertEquals(listOf(WireCallbackNumbers.Given("0312345678", 1_790_000_000, "<first@hotel.example>")), r.numbers)
    }

    @Test
    fun `a number the sender gave in another conversation is not pointed out`() {
        val m = message(2, "desk@hotel.example", "Call 03-1234-5678 or 0120-111-222")
        val history = listOf(WireCallbackNumbers.Given("0312345678", 1_790_000_000, "<first@hotel.example>"))
        assertEquals(listOf("0120-111-222"), CallbackNumbers.new(m, emptyList(), history))
    }

    @Test
    fun `a number first given by this very message is still pointed out`() {
        val m = message(2, "desk@hotel.example", "Call +81 3-1234-5678 or 0120-111-222")
        val history = listOf(
            WireCallbackNumbers.Given("81312345678", 1_790_000_100, "<2>"),
            WireCallbackNumbers.Given("0120111222", 1_790_000_000, "<older@hotel.example>"),
        )
        assertEquals(listOf("+81 3-1234-5678"), CallbackNumbers.new(m, emptyList(), history))
    }

    @Test
    fun `the history is compared by digits however the number is written`() {
        val m = message(2, "desk@hotel.example", "Call (03) 1234.5678 or 03 9999 0000")
        val history = listOf(WireCallbackNumbers.Given("0312345678", 1_790_000_000, "<first@hotel.example>"))
        assertEquals(listOf("03 9999 0000"), CallbackNumbers.new(m, emptyList(), history))
    }

    @Test
    fun `at most three are named, and an HTML-only message is read as text`() {
        assertEquals(3, CallbackNumbers.new(message(1, "x@y", "a 03-1111-1111 b 03-2222-2222 c 03-3333-3333 d 03-4444-4444"), emptyList()).size)
        assertEquals(
            listOf("+81 3-9999-0000"),
            CallbackNumbers.new(message(1, "x@y", null, "<p>Call <b>+81 3-9999-0000</b></p>"), emptyList()),
        )
    }
}
