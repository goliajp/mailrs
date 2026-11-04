package jp.golia.mailrs

import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.wire.RemoteImages
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The reader's own settings: each change reaches the stub as the request the
 * server takes, and what the page shows is read back from it. The same
 * journeys as the iPhone's `SettingsFlowTests`.
 */
@RunWith(AndroidJUnit4::class)
class SettingsFlowTest : MailrsUiTest() {

    /** The image setting is kept between launches; every test starts from the default. */
    @After
    fun imagesBackToDefault() {
        Prefs(InstrumentationRegistry.getInstrumentation().targetContext).remoteImages = RemoteImages.Click
    }

    private fun openSettingsPage(tag: String) {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performScrollTo().performClick()
        compose.onNodeWithTag(tag).performScrollTo().performClick()
    }

    /** The newest write to `path`, waited for. */
    private fun write(method: String, path: String): JSONObject? {
        var found: JSONObject? = null
        runCatching {
            compose.waitUntil(TIMEOUT_MS) {
                val all = JSONArray(readStub("/debug/settings"))
                found = (0 until all.length()).map { all.getJSONObject(it) }
                    .lastOrNull { it.getString("method") == method && it.getString("path") == path }
                found != null
            }
        }
        return found
    }

    private fun exists(tag: String) =
        compose.onAllNodes(hasTestTag(tag), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    private fun waitFor(what: String, check: () -> Boolean) {
        assertTrue(what, runCatching { compose.waitUntil(TIMEOUT_MS) { check() } }.isSuccess)
    }

    @Test
    fun the_notification_mode_and_quiet_hours_are_saved() {
        openSettingsPage("settings.page.Notifications")
        waitFor("the sender set apart from the rule was not listed") { exists("notify.sender.news@shop.test") }
        assertTrue("the muted conversation was not listed", exists("notify.thread.t1"))
        compose.onNodeWithTag("notify.mode.all").performClick()
        val mode = write("PUT", "/api/notify")
        assertEquals("the mode did not reach the server", "all", mode?.getJSONObject("body")?.getString("mode"))
        compose.onNodeWithTag("notify.quiet").performClick()
        waitFor("quiet hours did not reach the server") {
            write("PUT", "/api/notify")?.getJSONObject("body")?.has("quiet_start") == true
        }
        val quiet = write("PUT", "/api/notify")!!.getJSONObject("body")
        assertEquals(22 * 60, quiet.getInt("quiet_start"))
        assertEquals(7 * 60, quiet.getInt("quiet_end"))
        assertEquals("saving quiet hours lost the mode just chosen", "all", quiet.getString("mode"))
    }

    @Test
    fun other_devices_are_signed_out() {
        openSettingsPage("settings.page.Sessions")
        waitFor("the other device was not listed") { exists("session.s-laptop") }
        compose.onNodeWithTag("sessions.endOthers").performClick()
        assertNotNull("signing the others out never reached the server", write("POST", "/api/auth/sessions/end-others"))
        waitFor("the signed-out device stayed listed") { !exists("session.s-laptop") }
        assertTrue("this device was signed out with the others", exists("session.s-this"))
    }

    @Test
    fun an_app_password_is_revoked_and_new_ones_are_made_on_the_web() {
        openSettingsPage("settings.page.AppPasswords")
        waitFor("the app passwords were not listed") { exists("appPassword.ap-old") }
        assertTrue("no way to the web to make one", exists("appPasswords.web"))
        compose.onNodeWithTag("appPassword.ap-old.revoke").performClick()
        assertNotNull("the revoke never reached the server", write("DELETE", "/api/auth/app-passwords/ap-old"))
        waitFor("the revoked password stayed listed") { !exists("appPassword.ap-old") }
        assertTrue("revoking one took another with it", exists("appPassword.ap-tb"))
    }

    @Test
    fun the_vacation_reply_is_turned_on_with_its_text_kept() {
        openSettingsPage("settings.page.Vacation")
        waitFor("the vacation reply never loaded") { exists("vacation.enabled") }
        compose.onNodeWithTag("vacation.enabled").performClick()
        val body = write("PUT", "/api/settings/vacation")?.getJSONObject("body")
        assertEquals("turning it on did not reach the server", true, body?.getBoolean("enabled"))
        assertEquals("the reply's text was not sent back as read", "Back on Monday.", body?.getString("body"))
        assertEquals("the reply's interval was not sent back as read", 7, body?.getInt("days"))
    }

    @Test
    fun a_masked_address_is_created_and_listed() {
        openSettingsPage("settings.page.Masked")
        waitFor("the masked addresses were not listed") { exists("mask.m1") }
        compose.onNodeWithTag("masked.forDomain").performTextInput("news.test")
        compose.onNodeWithTag("masked.create").performClick()
        val body = write("POST", "/api/masked-emails")?.getJSONObject("body")
        assertEquals("the site did not reach the server", "news.test", body?.getString("forDomain"))
        waitFor("the new address was not listed") { exists("mask.m2") }
    }

    /** "Never load" keeps the note and takes the button away; MailFlowTest has the default's button. */
    @Test
    fun never_load_takes_the_load_button_away() {
        openSettingsPage("images.Never")
        compose.onNodeWithTag("button.closeSettings").performClick()
        waitForTag("list.conversations", "settings did not close")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        unfoldAll()
        waitFor("the remote image was not held back") { exists("body.remoteBlocked") }
        assertTrue("Never load still offered Load", !exists("button.loadImages"))
    }

    @Test
    fun a_senders_mail_is_always_filed_to_promotions() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        unfoldAll()
        compose.onNodeWithTag("message.settings.1").performScrollTo().performClick()
        compose.onNodeWithTag("sender.file.promotions").performClick()
        val body = write("POST", "/api/senders/reclassify")?.getJSONObject("body")
        assertEquals("the bucket did not reach the server", "promotions", body?.getString("bucket"))
        assertEquals("the sender's address did not reach the server", "alice@example.com", body?.getString("sender"))
    }

    @Test
    fun a_conversation_is_muted() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("message.settings.2", "the newest message had no menu")
        compose.onNodeWithTag("message.settings.2").performClick()
        compose.onNodeWithTag("thread.notify.mute").performClick()
        val body = write("PUT", "/api/notify/threads/t1")?.getJSONObject("body")
        assertEquals("muting did not reach the server", "mute", body?.getString("value"))
    }
}
