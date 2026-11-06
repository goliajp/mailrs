package jp.golia.mailrs.wire

import java.io.File
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

/**
 * A UnifiedPush ping says only "look": what the notification shows is
 * fetched afterwards, by the app, from its own server.
 */
class PushPingTest {

    @Before
    fun words() {
        TestTexts.install()
    }

    private fun row(threadId: String, from: String, subject: String, unread: Int, at: Long) = Wire.Conversation(
        threadId = threadId,
        subject = subject,
        participants = listOf(from),
        messageCount = 1,
        unreadCount = unread,
        lastDate = at,
        category = "inbox",
        flagged = false,
        snippet = "",
        pinned = false,
        archived = false,
        importanceLevel = "normal",
        importanceScore = 0f,
        requiresAction = false,
        receivedCount = 1,
        sentCount = 0,
    )

    /** The calls the check made, in order. */
    private val calls = mutableListOf<String>()

    private fun check(count: Int?, inbox: List<Wire.Conversation>?) = NewMailCheck(
        unseen = {
            calls += "unseen"
            count
        },
        inbox = {
            calls += "inbox"
            inbox
        },
    )

    @Test
    fun a_ping_fetches_the_count_then_the_inbox_and_names_the_newest_unread() = runBlocking {
        val inbox = listOf(
            row("t-old", "Bob <bob@x.test>", "Old", unread = 1, at = 10),
            row("t-new", "Alice Smith <alice@x.test>", "Quarterly report", unread = 1, at = 20),
            row("t-read", "Carol <carol@x.test>", "Read already", unread = 0, at = 30),
        )
        val found = check(count = 5, inbox = inbox).run(previous = 4)!!
        assertEquals(listOf("unseen", "inbox"), calls)
        assertEquals(5, found.count)
        val notice = found.notice!!
        assertEquals("Alice Smith", notice.title)
        assertEquals("Quarterly report", notice.text)
        assertEquals("t-new", notice.threadId)
        assertNull("one arrived: no count underneath", notice.summary)
    }

    @Test
    fun several_arrived_says_how_many_underneath() = runBlocking {
        val inbox = listOf(row("t1", "Alice <a@x.test>", "Hi", unread = 1, at = 1))
        val notice = check(count = 7, inbox = inbox).run(previous = 4)!!.notice!!
        assertEquals(NewMailRule.text(3), notice.summary)
    }

    /** A ping for mail already read elsewhere, or a duplicate ping: nothing is said, and the inbox is not fetched. */
    @Test
    fun a_ping_with_nothing_new_fetches_only_the_count_and_says_nothing() = runBlocking {
        val found = check(count = 4, inbox = emptyList()).run(previous = 4)!!
        assertEquals(listOf("unseen"), calls)
        assertNull(found.notice)
        assertNull(found.inbox)
    }

    @Test
    fun a_ping_when_the_server_cannot_be_reached_remembers_nothing() = runBlocking {
        assertNull(check(count = null, inbox = null).run(previous = 4))
        assertEquals(listOf("unseen"), calls)
    }

    /** The inbox did not come: the count still says what arrived. */
    @Test
    fun without_the_inbox_the_notice_says_how_many() = runBlocking {
        val notice = check(count = 6, inbox = null).run(previous = 4)!!.notice!!
        assertEquals("Mailrs", notice.title)
        assertEquals(NewMailRule.text(2), notice.text)
        assertNull(notice.threadId)
    }

    @Test
    fun the_registration_body_has_the_server_s_field_names() {
        val fixture = File("../../wire-contract/requests/unified-push-register.json").readText()
        val expected = Json.parseToJsonElement(fixture).jsonObject
        val keys = expected.getValue("keys").jsonObject
        val sent = Json.encodeToString(
            UnifiedPushWire.Register.serializer(),
            UnifiedPushWire.Register(
                endpoint = "https://ntfy.example.org/upJ8sk2LmQ0aZp?up=1",
                keys = UnifiedPushWire.Keys(p256dh = "k", auth = "a"),
                name = "Pixel 8",
                resume = true,
            ),
        )
        val body = Json.parseToJsonElement(sent).jsonObject
        assertEquals(expected.keys, body.keys)
        assertEquals(keys.keys, body.getValue("keys").jsonObject.keys)
    }
}
