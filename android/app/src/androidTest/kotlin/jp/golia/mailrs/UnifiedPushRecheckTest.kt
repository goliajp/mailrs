package jp.golia.mailrs

import android.os.ParcelFileDescriptor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.work.WorkManager
import jp.golia.mailrs.wire.PushDistributor
import jp.golia.mailrs.wire.PushRegistration
import jp.golia.mailrs.wire.PushSyncWorker
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.security.MessageDigest

/**
 * The server can forget a phone without telling it: another device took
 * its place under the per-account cap. The app finds out by reading the
 * server's list on launch and on the push settings page, and registers
 * again only when it is missing.
 */
@RunWith(AndroidJUnit4::class)
class UnifiedPushRecheckTest : MailrsUiTest() {

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val app = instrumentation.targetContext
    private val distributor = instrumentation.context.packageName

    // a package never opened is "stopped" and the connector's broadcasts do not reach it
    @Before
    fun theDistributorHasBeenOpened() {
        val (out, input, err) = instrumentation.uiAutomation.executeShellCommandRwe(
            "am start -W -n $distributor/${FakeDistributorOpen::class.java.name}",
        )
        input.close()
        val said = ParcelFileDescriptor.AutoCloseInputStream(out).use { String(it.readBytes()) } +
            ParcelFileDescriptor.AutoCloseInputStream(err).use { String(it.readBytes()) }
        assertTrue("the distributor did not open: $said", said.contains("Status: ok"))
    }

    private fun waitFor(what: String, check: () -> Boolean) {
        assertTrue(what, runCatching { compose.waitUntil(TIMEOUT_MS) { check() } }.isSuccess)
    }

    /** WorkManager and the stub are not frames: a plain wait. */
    private fun waitOutside(what: String, check: () -> Boolean) {
        val deadline = System.currentTimeMillis() + TIMEOUT_MS
        while (!check() && System.currentTimeMillis() < deadline) Thread.sleep(100)
        assertTrue(what, check())
    }

    private fun registrations(): List<JSONObject> {
        val all = JSONArray(readStub("/debug/unifiedpush"))
        return (0 until all.length()).map { all.getJSONObject(it) }
            .filter { it.getString("method") == "POST" && it.getString("path") == "/api/push/unifiedpush" }
    }

    private fun reads() = JSONObject(readStub("/debug/unifiedpush-reads")).getInt("list")

    private fun syncFinished() =
        WorkManager.getInstance(app).getWorkInfosForUniqueWork(PushSyncWorker.WORK_NAME).get().all { it.state.isFinished }

    private fun idOf(endpoint: String) =
        MessageDigest.getInstance("SHA-256").digest(endpoint.toByteArray()).joinToString("") { "%02x".format(it) }

    private fun evict(id: String) {
        val c = java.net.URL(StubServer.base() + "/debug/unifiedpush-evict/$id").openConnection() as java.net.HttpURLConnection
        c.requestMethod = "POST"
        val said = c.inputStream.use { String(it.readBytes()) }
        assertEquals("the stub did not have this phone to forget", true, JSONObject(said).getBoolean("evicted"))
    }

    private fun openPushPage() {
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performScrollTo().performClick()
        compose.onNodeWithTag("settings.page.Push").performScrollTo().performClick()
    }

    /** Signed in and push turned on; the endpoint the server was given. */
    private fun turnedOn(): String {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        openPushPage()
        val label = PushDistributor.label(app, distributor)
        waitFor("the page did not offer to turn on") {
            compose.onAllNodes(hasTestTag("push.turnOn"), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithTag("push.turnOn").performClick()
        waitFor("the page did not say push is on") {
            compose.onAllNodes(hasTestTag("push.status").and(hasText(app.getString(R.string.push_status_on, label))), useUnmergedTree = true)
                .fetchSemanticsNodes().isNotEmpty()
        }
        waitOutside("the sync did not finish") { syncFinished() }
        assertEquals("one registration to start with", 1, registrations().size)
        return registrations().single().getJSONObject("body").getString("endpoint")
    }

    /** The app opened again from the launcher, while the server keeps its state. */
    private fun launchAgain() {
        val before = reads()
        ActivityScenario.launch(MainActivity::class.java).use {
            waitOutside("the launch never read the server's list") { reads() > before }
            waitOutside("the check did not finish") { syncFinished() }
        }
    }

    @Test
    fun a_phone_the_server_forgot_registers_again_on_launch() {
        val endpoint = turnedOn()
        evict(idOf(endpoint))

        launchAgain()

        waitOutside("the forgotten phone did not register again") { registrations().size == 2 }
        assertEquals(endpoint, registrations().last().getJSONObject("body").getString("endpoint"))
        assertTrue("the server still does not list it", readStub("/api/push/unifiedpush").contains(idOf(endpoint)))
    }

    /** Removed by a person from another device: the stub keeps it out until it resumes. */
    private fun removeElsewhere(id: String) {
        val c = java.net.URL(StubServer.base() + "/debug/unifiedpush-remove/$id").openConnection() as java.net.HttpURLConnection
        c.requestMethod = "POST"
        val said = c.inputStream.use { String(it.readBytes()) }
        assertEquals("the stub did not have this phone to remove", true, JSONObject(said).getBoolean("removed"))
    }

    private fun shown(tag: String) =
        compose.onAllNodes(hasTestTag(tag), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun a_phone_removed_from_another_device_turns_off_on_launch_instead_of_registering_again() {
        val endpoint = turnedOn()
        removeElsewhere(idOf(endpoint))

        launchAgain()

        waitOutside("the launch did not turn pushes off here") { PushRegistration(app).removedElsewhere }
        assertEquals("registered again although a person removed it", 1, registrations().size)
        assertTrue("pushes are still on here", PushRegistration(app).snapshot().endpoint == null)
    }

    @Test
    fun a_phone_removed_from_another_device_says_so_and_comes_back_only_when_turned_on() {
        val endpoint = turnedOn()
        assertTrue("turning on did not say so", registrations().single().getJSONObject("body").getBoolean("resume"))
        assertTrue("the page does not say removing is not signing out", shown("push.removeIsNotSignOut"))
        assertTrue("the page says removed before anything was", !shown("push.removedElsewhere"))
        removeElsewhere(idOf(endpoint))

        compose.onNodeWithTag("button.closeSettingsPage").performClick()
        scrollToTag("settings.page.Push", "closing the page did not return to settings")
        compose.onNodeWithTag("settings.page.Push").performClick()
        waitFor("the page did not say the phone was removed elsewhere") { shown("push.removedElsewhere") }
        assertTrue("the page does not offer to turn on again", shown("push.turnOn"))
        assertEquals("opening the page registered it again", 1, registrations().size)

        // the reader turning it on again is what brings it back
        compose.onNodeWithTag("push.turnOn").performClick()
        waitFor("turning on again did not register") { registrations().size == 2 }
        assertTrue("turning on again did not say so", registrations().last().getJSONObject("body").getBoolean("resume"))
        waitFor("the removed line stayed after turning on again") { !shown("push.removedElsewhere") }
        val listed = JSONObject(readStub("/api/push/unifiedpush"))
        val devices = listed.getJSONArray("devices")
        // the distributor may hand out a new endpoint once unregistered: the phone is listed under that one
        val now = registrations().last().getJSONObject("body").getString("endpoint")
        assertTrue("the server does not list it again", (0 until devices.length()).any { devices.getJSONObject(it).getString("id") == idOf(now) })
        assertTrue("the server still has it as removed", !listed.getJSONArray("removed").toString().contains(idOf(now)))
    }

    @Test
    fun a_phone_the_server_still_lists_does_not_register_again_on_launch() {
        turnedOn()

        launchAgain()

        assertEquals("registered again although the server had it", 1, registrations().size)
    }

    @Test
    fun opening_the_push_page_registers_a_forgotten_phone_and_says_when() {
        val endpoint = turnedOn()
        assertTrue(
            "the page showed a re-registration before one happened",
            compose.onAllNodes(hasTestTag("push.reregistered"), useUnmergedTree = true).fetchSemanticsNodes().isEmpty(),
        )
        evict(idOf(endpoint))

        compose.onNodeWithTag("button.closeSettingsPage").performClick()
        scrollToTag("settings.page.Push", "closing the page did not return to settings")
        compose.onNodeWithTag("settings.page.Push").performClick()

        // the page reads the list in a composition effect: the compose clock has to run
        waitFor("opening the page did not register the forgotten phone") { registrations().size == 2 }
        waitFor("the page did not say the phone was registered again") {
            compose.onAllNodes(hasTestTag("push.reregistered"), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
        }
        assertTrue("this phone is not listed again", compose.onAllNodes(hasTestTag("push.device.${idOf(endpoint)}"), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty())
    }
}
