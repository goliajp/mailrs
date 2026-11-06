package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Test

/** What the server is told so it holds what the distributor gave, and what the page says. */
class PushSyncTest {

    private val none = PushSync.Snapshot(null, null, null, null, null, null)

    private fun given(endpoint: String) = none.copy(endpoint = endpoint, p256dh = "k", auth = "a")

    private fun held(s: PushSync.Snapshot, id: String) =
        s.copy(serverId = id, serverEndpoint = s.endpoint, serverSent = PushSync.sent(s.endpoint!!, s.p256dh!!, s.auth!!))

    @Test
    fun a_new_endpoint_is_registered() {
        assertEquals(listOf(PushSync.Step.Register("https://p.test/up1", "k", "a")), PushSync.steps(given("https://p.test/up1")))
    }

    @Test
    fun what_the_server_holds_needs_nothing() {
        assertEquals(emptyList<PushSync.Step>(), PushSync.steps(held(given("https://p.test/up1"), "id1")))
        assertEquals(emptyList<PushSync.Step>(), PushSync.steps(none))
    }

    /** The old endpoint reaches nobody once the distributor replaced it. */
    @Test
    fun a_replaced_endpoint_drops_the_old_one_first() {
        val replaced = held(given("https://p.test/up1"), "id1").copy(endpoint = "https://p.test/up2")
        assertEquals(
            listOf(PushSync.Step.Drop("id1"), PushSync.Step.Register("https://p.test/up2", "k", "a")),
            PushSync.steps(replaced),
        )
    }

    @Test
    fun new_keys_for_the_same_endpoint_are_sent_again_without_a_drop() {
        val rekeyed = held(given("https://p.test/up1"), "id1").copy(p256dh = "k2")
        assertEquals(listOf(PushSync.Step.Register("https://p.test/up1", "k2", "a")), PushSync.steps(rekeyed))
    }

    @Test
    fun turned_off_drops_what_the_server_holds() {
        val off = held(given("https://p.test/up1"), "id1").copy(endpoint = null, p256dh = null, auth = null)
        assertEquals(listOf(PushSync.Step.Drop("id1")), PushSync.steps(off))
    }

    @Test
    fun the_page_says_where_it_stands() {
        val on = held(given("https://p.test/up1"), "id1")
        val pkg = "io.heckel.ntfy"
        assertEquals(PushStatus.ServerOff, PushStatus.of(false, listOf(pkg), pkg, on, null))
        assertEquals(PushStatus.NoDistributor, PushStatus.of(true, emptyList(), null, none, null))
        assertEquals(PushStatus.Off, PushStatus.of(true, listOf(pkg), null, none, null))
        assertEquals(PushStatus.Waiting(pkg), PushStatus.of(true, listOf(pkg), pkg, none, null))
        assertEquals(PushStatus.Waiting(pkg), PushStatus.of(true, listOf(pkg), pkg, given("https://p.test/up1"), null))
        assertEquals(PushStatus.On(pkg), PushStatus.of(true, listOf(pkg), pkg, on, null))
        assertEquals(PushStatus.Failed(pkg, "NETWORK"), PushStatus.of(true, listOf(pkg), pkg, none, "NETWORK"))
    }
}
