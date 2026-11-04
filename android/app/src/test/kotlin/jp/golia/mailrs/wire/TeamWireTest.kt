package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The same cases as the iPhone's `TeamWireTests`. */
class TeamWireTest {
    @Test
    fun a_shared_conv_is_two_numbers() {
        assertEquals(TeamWire.Conv(1, 7), TeamWire.Conv.of("1:7"))
        assertEquals("/api/shared/1/7", TeamWire.Conv.of("1:7")!!.path)
        assertNull(TeamWire.Conv.of(""))
        assertNull(TeamWire.Conv.of("1"))
        assertNull(TeamWire.Conv.of("a:7"))
        assertNull(TeamWire.Conv.of("1:7:9"))
    }

    @Test
    fun someone_else_writing_is_said_before_who_is_assigned() {
        val o = TeamWire.Overview("bob@golia.jp", TeamWire.DraftPointer("carol@golia.jp"))
        assertEquals(TeamWire.ReplyHint.Writing("carol"), TeamWire.replyHint(o, "me@golia.jp"))
    }

    @Test
    fun my_own_draft_falls_through_to_the_assignee() {
        val o = TeamWire.Overview("bob@golia.jp", TeamWire.DraftPointer("Me@Golia.jp"))
        assertEquals(TeamWire.ReplyHint.Assigned("bob"), TeamWire.replyHint(o, "me@golia.jp"))
    }

    @Test
    fun mine_or_nobody_says_nothing() {
        assertNull(TeamWire.replyHint(TeamWire.Overview("me@golia.jp", null), "me@golia.jp"))
        assertNull(TeamWire.replyHint(TeamWire.Overview(null, null), "me@golia.jp"))
        assertNull(TeamWire.replyHint(TeamWire.Overview("", TeamWire.DraftPointer("")), "me@golia.jp"))
    }
}
