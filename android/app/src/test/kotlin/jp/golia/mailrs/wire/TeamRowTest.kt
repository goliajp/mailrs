package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The same cases as the iPhone's `TeamRowTests`. */
class TeamRowTest {
    private fun row(assignee: String) = Wire.Conversation(
        threadId = "t",
        subject = "s",
        participants = emptyList(),
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
        assignee = assignee,
    )

    @Test
    fun nobodyAssignedSaysNothing() {
        assertNull(TeamRow.assignee(row("")))
        assertNull(TeamRow.assignee(row("   ")))
    }

    @Test
    fun anAddressReadsAsItsLocalPart() {
        assertEquals("bob", TeamRow.assignee(row("bob@golia.jp")))
    }

    @Test
    fun aNameWithoutAnAtIsShownAsIs() {
        assertEquals("bob", TeamRow.assignee(row("bob")))
        assertEquals("@odd", TeamRow.assignee(row("@odd")))
    }
}
