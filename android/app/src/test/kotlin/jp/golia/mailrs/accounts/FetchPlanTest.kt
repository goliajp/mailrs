package jp.golia.mailrs.accounts

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Test

/** What to ask a folder for, given what is already held. */
class FetchPlanTest {
    @Test
    fun `a folder never read is read whole`() {
        assertEquals(FetchPlan.Everything, FetchPlan.decide(null, 1))
        assertEquals("1:*", FetchPlan.Everything.range)
    }

    // The next uid, not the last one — asking from the last would fetch
    // the newest message again on every pass.
    @Test
    fun `a folder read before asks for what came after`() {
        val plan = FetchPlan.decide(FolderMark(7, 4390), 7)
        assertEquals(FetchPlan.Since(4390), plan)
        assertEquals("4391:*", plan.range)
    }

    // **The one every client gets wrong once.** A changed UIDVALIDITY
    // means uid 4390 is not the message it was, so "everything after
    // 4390" skips mail or fetches the wrong thing.
    @Test
    fun `a renumbered folder is read from the start`() {
        val plan = FetchPlan.decide(FolderMark(7, 4390), 8)
        assertEquals(FetchPlan.Renumbered, plan)
        assertEquals("a renumbered folder was resumed from a stale uid", "1:*", plan.range)
    }

    @Test
    fun `a mark with no uid is not a resume point`() {
        assertEquals(FetchPlan.Everything, FetchPlan.decide(FolderMark(7, 0), 7))
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
