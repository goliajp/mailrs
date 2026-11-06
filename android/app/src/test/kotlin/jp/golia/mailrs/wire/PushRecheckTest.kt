package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Test

/** Whether the server still has this phone, read from what it lists. */
class PushRecheckTest {

    private val none = PushSync.Snapshot(null, null, null, null, null, null)

    private val on = PushSync.Snapshot(
        endpoint = "https://p.test/up1",
        p256dh = "k",
        auth = "a",
        serverId = "id1",
        serverEndpoint = "https://p.test/up1",
        serverSent = PushSync.sent("https://p.test/up1", "k", "a"),
    )

    private val other = UnifiedPushWire.Device(id = "d-old", name = "Old Moto")

    private fun listing(vararg ids: String) =
        UnifiedPushWire.State(enabled = true, devices = listOf(other) + ids.map { UnifiedPushWire.Device(id = it) })

    @Test
    fun listed_needs_nothing() {
        assertEquals(emptyList<PushSync.Step>(), PushSync.recheck(on, listing("id1")))
    }

    @Test
    fun missing_from_the_list_is_registered_again() {
        assertEquals(listOf(PushSync.Step.Register("https://p.test/up1", "k", "a")), PushSync.recheck(on, listing()))
    }

    @Test
    fun a_failed_read_needs_nothing() {
        assertEquals(emptyList<PushSync.Step>(), PushSync.recheck(on, null))
    }

    @Test
    fun push_off_here_needs_nothing() {
        assertEquals(emptyList<PushSync.Step>(), PushSync.recheck(none, listing()))
        val turnedOff = on.copy(endpoint = null, p256dh = null, auth = null)
        assertEquals(emptyList<PushSync.Step>(), PushSync.recheck(turnedOff, listing()))
    }

    /** Not yet told the server: the ordinary sync registers it, not the check. */
    @Test
    fun not_yet_registered_needs_nothing() {
        val given = none.copy(endpoint = "https://p.test/up1", p256dh = "k", auth = "a")
        assertEquals(emptyList<PushSync.Step>(), PushSync.recheck(given, listing()))
    }

    @Test
    fun a_server_with_push_off_needs_nothing() {
        assertEquals(emptyList<PushSync.Step>(), PushSync.recheck(on, UnifiedPushWire.State(enabled = false)))
    }
}
