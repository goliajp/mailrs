package jp.golia.mailrs

import android.app.Notification
import android.app.NotificationManager
import android.os.ParcelFileDescriptor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import jp.golia.mailrs.wire.NewMailWorker
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.wire.PushDistributor
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Before
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Instant notifications through a UnifiedPush distributor, against the
 * stub: turned on, the endpoint and keys reach the server; a ping makes the
 * app fetch and notify; turned off, the server forgets the phone. The
 * distributor is [FakeDistributor], installed with the test APK.
 */
@RunWith(AndroidJUnit4::class)
class UnifiedPushFlowTest : MailrsUiTest() {

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val app = instrumentation.targetContext
    private val distributor = instrumentation.context.packageName

    /**
     * Open the distributor once, as a person does after installing one: a
     * package never opened is "stopped", and broadcasts do not reach it.
     * `-W` waits for the activity, which finishes at once.
     */
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

    @After
    fun distributorBackAndShadeEmpty() {
        fakeDistributor(on = true)
        app.getSystemService(NotificationManager::class.java).cancel(NewMailWorker.NOTIFICATION_ID)
    }

    /**
     * Install or remove the fake distributor, and wait until the app sees
     * it. A system state, not a frame: a plain wait, not the compose clock.
     */
    private fun fakeDistributor(on: Boolean) {
        FakeDistributorSwitch.set(instrumentation.context, on)
        val deadline = System.currentTimeMillis() + TIMEOUT_MS
        while (PushDistributor.installed(app).contains(distributor) != on && System.currentTimeMillis() < deadline) {
            Thread.sleep(100)
        }
        assertEquals("the app did not see the fake distributor installed=$on", on, PushDistributor.installed(app).contains(distributor))
    }

    private fun openPushPage() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performScrollTo().performClick()
        compose.onNodeWithTag("settings.page.Push").performScrollTo().performClick()
    }

    private fun exists(tag: String) =
        compose.onAllNodes(hasTestTag(tag), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    private fun says(tag: String, text: String) =
        compose.onAllNodes(hasTestTag(tag).and(hasText(text)), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    private fun waitFor(what: String, check: () -> Boolean) {
        assertTrue(what, runCatching { compose.waitUntil(TIMEOUT_MS) { check() } }.isSuccess)
    }

    private fun writes(): List<JSONObject> {
        val all = JSONArray(readStub("/debug/unifiedpush"))
        return (0 until all.length()).map { all.getJSONObject(it) }
    }

    private fun write(method: String, path: String): JSONObject? =
        writes().lastOrNull { it.getString("method") == method && it.getString("path") == path }

    private fun posted(): Notification? =
        app.getSystemService(NotificationManager::class.java).activeNotifications
            .firstOrNull { it.id == NewMailWorker.NOTIFICATION_ID }?.notification

    @Test
    fun turned_on_the_phone_registers_a_ping_notifies_and_turned_off_it_is_forgotten() {
        openPushPage()
        waitFor("the phone registered elsewhere was not listed") { exists("push.device.d-old") }
        val label = PushDistributor.label(app, distributor)
        waitFor("the page did not say push is off") { says("push.status", app.getString(R.string.push_status_off)) }

        compose.onNodeWithTag("push.turnOn").performClick()
        waitFor("the endpoint never reached the server") { write("POST", "/api/push/unifiedpush") != null }
        val body = write("POST", "/api/push/unifiedpush")!!.getJSONObject("body")
        val endpoint = body.getString("endpoint")
        assertTrue(endpoint, endpoint.startsWith(FakeDistributor.ENDPOINT_PREFIX))
        val keys = body.getJSONObject("keys")
        assertEquals("the key is a P-256 point, base64url", 87, keys.getString("p256dh").length)
        assertEquals("the auth secret is 16 bytes, base64url", 22, keys.getString("auth").length)
        assertTrue("the phone has no name", body.getString("name").isNotBlank())
        waitFor("the page did not say push is on") { says("push.status", app.getString(R.string.push_status_on, label)) }
        assertTrue("the other phone left the list", exists("push.device.d-old"))

        // the ping carries nothing: what the notification says is fetched
        Prefs(app).lastUnseen = 1
        FakeDistributor.ping(instrumentation.context, app.packageName, endpoint, "new-mail".toByteArray())
        waitFor("the ping posted nothing") { posted() != null }
        assertEquals("Alice Smith", posted()!!.extras.getCharSequence(Notification.EXTRA_TITLE)?.toString())

        compose.onNodeWithTag("push.device.d-old.remove").performScrollTo().performClick()
        waitFor("removing the other phone did not reach the server") {
            write("DELETE", "/api/push/unifiedpush/d-old") != null
        }

        compose.onNodeWithTag("push.turnOff").performScrollTo().performClick()
        waitFor("turning off did not tell the server") {
            writes().any { it.getString("method") == "DELETE" && it.getString("path") != "/api/push/unifiedpush/d-old" }
        }
        waitFor("the page did not say push is off again") { says("push.status", app.getString(R.string.push_status_off)) }
    }

    @Test
    fun without_a_distributor_the_page_says_what_happens_instead() {
        fakeDistributor(on = false)
        assertTrue("the app still finds a distributor: ${PushDistributor.installed(app)}", PushDistributor.installed(app).isEmpty())
        openPushPage()
        waitFor("the page did not explain there is no distributor") { exists("push.noDistributor") }
        assertTrue(says("push.noDistributor", app.getString(R.string.push_none_installed)))
        assertTrue("it offered to turn on with nothing to turn on", !exists("push.turnOn"))
    }

    @Test
    fun a_server_without_push_says_so() {
        val c = java.net.URL(StubServer.base() + "/debug/unifiedpush-off").openConnection() as java.net.HttpURLConnection
        c.requestMethod = "POST"
        c.inputStream.use { it.readBytes() }
        openPushPage()
        waitFor("the page did not say the server has push off") { exists("push.serverOff") }
        assertTrue("it offered to turn on against a server that cannot push", !exists("push.turnOn"))
    }
}
