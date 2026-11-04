package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same cases as the iPhone's `InboxSectionsTests`. */
class InboxSectionsTest {
    private fun row(participants: List<String>) = Wire.Conversation(
        threadId = "t",
        subject = "s",
        participants = participants,
        messageCount = 1,
        unreadCount = 0,
        lastDate = 0,
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

    @Test
    fun aSectionIsAskedForByName() {
        val axes = MailList.Inbox.axes.copy(section = InboxSection.Awaiting.wire)
        assertTrue(axes.query().split("&").contains("section=awaiting"))
    }

    @Test
    fun theWholeInboxSendsNoSection() {
        assertFalse(MailList.Inbox.axes.query().contains("section="))
    }

    /** Each row's last participant who is not the reader, once each. */
    @Test
    fun acceptTakesEachRowsLatestOtherSender() {
        val rows = listOf(
            row(listOf("Ann <ANN@x.test>", "me@golia.jp")),
            row(listOf("bob@y.test")),
            row(listOf("Ann <ann@x.test>")),
            row(listOf("me@golia.jp")),
        )
        assertEquals(listOf("ann@x.test", "bob@y.test"), InboxSection.latestSenders(rows, "Me@golia.jp"))
    }
}
