package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Filing a message, on servers that differ about how.
 *
 * The assertion that earns its place is the last one: **a server
 * without UIDPLUS is never sent a bare EXPUNGE**, because that would
 * remove every message in the folder somebody else's client had
 * flagged — mail this app never saw and cannot bring back.
 */
class MovePlanTest {
    private fun texts(vararg caps: String) =
        MovePlan.steps(7L, "Trash", caps.toSet()).mapNotNull {
            when (it) {
                is MovePlan.Step.Command -> it.text
                MovePlan.Step.MarkDeleted -> null
            }
        }

    @Test
    fun `a server with MOVE is asked once`() {
        assertEquals(listOf("UID MOVE 7 \"Trash\""), texts("MOVE", "UIDPLUS"))
    }

    @Test
    fun `without MOVE the message is copied then flagged`() {
        val steps = MovePlan.steps(7L, "Trash", setOf("UIDPLUS"))
        assertEquals(MovePlan.Step.Command("UID COPY 7 \"Trash\""), steps[0])
        assertEquals(MovePlan.Step.MarkDeleted, steps[1])
        assertEquals(MovePlan.Step.Command("UID EXPUNGE 7"), steps[2])
    }

    // **The one that matters.** A bare EXPUNGE takes every \Deleted
    // message in the folder, including ones another client flagged and
    // has not expunged. Flagged-and-left disappears from the list just
    // the same and takes nothing with it.
    @Test
    fun `a server without UIDPLUS is never sent an expunge`() {
        val plain = texts()
        assertTrue("the copy was not made", plain.any { it.startsWith("UID COPY") })
        assertFalse("an expunge was sent", plain.any { it.contains("EXPUNGE") })
        assertTrue(
            "the message was not flagged, so it would stay in both folders",
            MovePlan.steps(7L, "Trash", emptySet()).contains(MovePlan.Step.MarkDeleted),
        )
    }

    // A folder name with a space or a quote is a name, not syntax.
    @Test
    fun `an awkward folder name is quoted`() {
        val steps = MovePlan.steps(1L, "[Gmail]/All Mail", setOf("MOVE"))
        assertEquals(
            listOf(MovePlan.Step.Command("UID MOVE 1 \"[Gmail]/All Mail\"")),
            steps,
        )
    }
}
