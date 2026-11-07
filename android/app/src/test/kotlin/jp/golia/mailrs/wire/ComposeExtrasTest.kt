package jp.golia.mailrs.wire

import java.time.Instant
import java.time.ZoneOffset
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The composer's From list, Undo hold, formatting, templates and kept schedule. */
class ComposeExtrasTest {
    // the client's own settings, so defaults are left out the way they are sent
    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }

    @Test
    fun `bold wraps the selection and wrapping again takes it off`() {
        val on = MarkdownFormat.apply("say hi now", 4, 6, MarkdownFormat.Kind.Bold)
        assertEquals(MarkdownFormat.Edit("say **hi** now", 6, 8), on)
        assertEquals(MarkdownFormat.Edit("say hi now", 4, 6), MarkdownFormat.apply(on.value, 6, 8, MarkdownFormat.Kind.Bold))
    }

    @Test
    fun `italic uses underscores and link selects the address`() {
        assertEquals("say _hi_", MarkdownFormat.apply("say hi", 4, 6, MarkdownFormat.Kind.Italic).value)
        val link = MarkdownFormat.apply("see docs", 4, 8, MarkdownFormat.Kind.Link)
        assertEquals("see [docs](https://)", link.value)
        assertEquals("https://", link.value.substring(link.start, link.end))
    }

    @Test
    fun `bold with nothing selected leaves the caret between the marks`() {
        assertEquals(MarkdownFormat.Edit("Hello ****", 8, 8), MarkdownFormat.apply("Hello ", 6, 6, MarkdownFormat.Kind.Bold))
    }

    @Test
    fun `own address first, without repeats`() {
        assertEquals(
            listOf("me@golia.jp", "sales@golia.jp"),
            SendAddresses.list("me@golia.jp", listOf("sales@golia.jp", "ME@golia.jp", " ")),
        )
        val options = listOf("me@golia.jp", "sales@golia.jp")
        assertEquals("sales@golia.jp", SendAddresses.replyFrom(options, "Sales <sales@golia.jp>"))
        assertEquals("me@golia.jp", SendAddresses.replyFrom(options, "other@example.com"))
    }

    @Test
    fun `the role reads send_as and tolerates its absence`() {
        val full = json.decodeFromString(Role.serializer(), """{"is_admin": false, "address": "me@golia.jp", "send_as": ["sales@golia.jp"]}""")
        assertEquals(listOf("sales@golia.jp"), full.sendAs)
        assertEquals("me@golia.jp", full.address)
        assertTrue(json.decodeFromString(Role.serializer(), """{"is_admin": true}""").sendAs.isEmpty())
    }

    @Test
    fun `a send asks the server to hold it`() {
        val body = json.encodeToString(
            SendRequest.serializer(),
            SendRequest(to = listOf("a@b.jp"), subject = "s", body = "b", undoSeconds = SendHold.SECONDS),
        )
        assertEquals("10", json.parseToJsonElement(body).jsonObject["undo_seconds"]?.jsonPrimitive?.content)
        assertTrue("the offer must end before the hold", SendHold.OFFERED_MS < SendHold.SECONDS * 1000L)
    }

    @Test
    fun `a draft carries its address and time both ways`() {
        val draft = json.decodeFromString(
            ServerDraft.serializer(),
            """{"id": 3, "subject": "s", "from": "sales@golia.jp", "scheduled_at": 1900000000}""",
        )
        assertEquals("sales@golia.jp", draft.from)
        assertEquals(1_900_000_000L, draft.scheduledAt)
        val saved = json.encodeToString(
            SaveDraftRequest.serializer(),
            SaveDraftRequest(to = "a@b.jp", cc = "", bcc = "", subject = "s", body = "b", from = "sales@golia.jp", scheduledAt = 1_900_000_000L),
        )
        val obj = json.parseToJsonElement(saved).jsonObject
        assertEquals("sales@golia.jp", obj["from"]?.jsonPrimitive?.content)
        assertEquals("1900000000", obj["scheduled_at"]?.jsonPrimitive?.content)
    }

    @Test
    fun `plain send keeps a future time and drops a past one`() {
        val now = Instant.ofEpochSecond(1_800_000_000).atZone(ZoneOffset.UTC)
        assertEquals(1_800_000_600L, SendSchedule.Now.withKept(1_800_000_600L, now))
        assertNull(SendSchedule.Now.withKept(1_799_999_000L, now))
        assertNull(SendSchedule.Now.withKept(null, now))
        assertEquals(1_800_010_800L, SendSchedule.LaterToday.withKept(1_900_000_000L, now))
    }

    @Test
    fun `a template fills what it has and a reply keeps its subject`() {
        val thanks = Template(1, "Thanks", "Thank you", "Thank you.")
        assertEquals(TemplateApply.Result("Thank you", "Thank you."), TemplateApply.apply(thanks, "", "old", isReply = false))
        assertEquals("Re: x", TemplateApply.apply(thanks, "Re: x", "", isReply = true).subject)
        val bodyOnly = Template(2, "Sig", "", "Best")
        assertEquals(TemplateApply.Result("Mine", "Best"), TemplateApply.apply(bodyOnly, "Mine", "", isReply = false))
    }
}
