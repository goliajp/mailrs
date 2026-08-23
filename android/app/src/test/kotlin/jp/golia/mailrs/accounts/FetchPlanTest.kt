package jp.golia.mailrs.accounts

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** What to ask a folder for, given what is already held. */
class FetchPlanTest {
    /**
     * **A window, not the whole folder.** A first sync of a mailbox
     * with fifty thousand messages would fetch fifty thousand header
     * blocks — hundreds of megabytes, many minutes, and a row list far
     * past what the device stores in one go.
     */
    @Test
    fun `a folder never read is read from its end`() {
        val plan = FetchPlan.decide(null, 1, exists = 50_000, window = 500)
        assertEquals("49501:*", plan.range)
    }

    /**
     * **By position, not by uid.** "The last five hundred messages" is
     * what a sequence number means; there is no uid arithmetic that
     * says it, because uids have gaps wherever anything was deleted.
     * `UID FETCH 1:500` and `FETCH 1:500` are different questions.
     */
    @Test
    fun `a first pass counts positions and a resume counts uids`() {
        assertFalse(FetchPlan.decide(null, 1, exists = 50_000).byUid)
        assertTrue(FetchPlan.decide(FolderMark(7, 4390), 7).byUid)
    }

    /** A folder smaller than the window is read whole. */
    @Test
    fun `a small folder is read whole`() {
        assertEquals("1:*", FetchPlan.decide(null, 1, exists = 12, window = 500).range)
        assertEquals("1:*", FetchPlan.decide(null, 1, exists = 0, window = 500).range)
    }

    // The next uid, not the last one — asking from the last would fetch
    // the newest message again on every pass.
    @Test
    fun `a folder read before asks for what came after`() {
        val plan = FetchPlan.decide(FolderMark(7, 4390), 7)
        assertEquals(FetchPlan.Since(4390), plan)
        assertEquals("4391:*", plan.range)
    }

    /**
     * **The one every client gets wrong once.** A changed UIDVALIDITY
     * means uid 4390 is not the message it was, so "everything after
     * 4390" skips mail or fetches the wrong thing — and the folder is
     * read from its end again, by position, like a first pass.
     */
    @Test
    fun `a renumbered folder is read again from its end`() {
        val plan = FetchPlan.decide(FolderMark(7, 4390), 8, exists = 50_000, window = 500)
        assertTrue(plan.toString(), plan is FetchPlan.Renumbered)
        assertEquals("a renumbered folder was resumed from a stale uid", "49501:*", plan.range)
        assertFalse("a renumbered folder was asked by uid", plan.byUid)
    }

    @Test
    fun `a mark with no uid is not a resume point`() {
        val plan = FetchPlan.decide(FolderMark(7, 0), 7, exists = 12)
        assertTrue(plan.toString(), plan is FetchPlan.Newest)
    }

    // The validity travels **with** the uid. Stored apart they drift,
    // and a uid without the validity that issued it means nothing.
    @Test
    fun `the mark carries both or neither`() {
        val mark = FolderMark(7, 4390)
        val back = Json.decodeFromString(
            FolderMark.serializer(),
            Json.encodeToString(FolderMark.serializer(), mark),
        )
        assertEquals(mark, back)
    }
}
