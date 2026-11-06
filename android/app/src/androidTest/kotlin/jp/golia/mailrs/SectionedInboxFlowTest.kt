package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The section tabs over the Inbox follow the reader's sectioned inbox
 * setting, the one the web and the iPhone read too: on until it is turned
 * off, off in Settings saves on the server and takes the tabs away, and off
 * on the server keeps them away. The same journeys as the iPhone's
 * `SectionedInboxFlowTests`.
 */
@RunWith(AndroidJUnit4::class)
class SectionedInboxFlowTest : MailrsUiTest() {

    private fun awaiting(complaint: String, condition: () -> Boolean) {
        runCatching { compose.waitUntil(TIMEOUT_MS, condition) }
            .onFailure { throw AssertionError(complaint) }
    }

    private fun tabsShown() = compose.onAllNodesWithTag("section.all").fetchSemanticsNodes().isNotEmpty()

    @Test
    fun turning_the_setting_off_hides_the_tabs_and_saves_on_the_server() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        awaiting("no section tabs over the Inbox while the setting is on") { tabsShown() }

        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performScrollTo().performClick()
        compose.onNodeWithTag("switch.sectionedInbox").performScrollTo().performClick()
        var saved = ""
        awaiting("turning it off did not reach the server: $saved") {
            val all = JSONArray(readStub("/debug/settings"))
            val last = (0 until all.length()).map { all.getJSONObject(it) }
                .lastOrNull { it.getString("path") == "/api/settings/sectioned-inbox" }
            saved = last?.toString().orEmpty()
            last?.getJSONObject("body")?.optBoolean("enabled", true) == false
        }
        compose.onNodeWithTag("button.closeSettings").performClick()
        waitForTag("list.conversations", "the inbox did not come back")
        awaiting("the section tabs stayed after the setting was turned off") { !tabsShown() }
    }

    @Test
    fun off_on_the_server_keeps_the_tabs_away() {
        // turned off on another device
        putSetting(enabled = false)
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        awaiting("the tabs showed though the server says off") { !tabsShown() }
    }

    private fun putSetting(enabled: Boolean) {
        val c = java.net.URL(stubBase() + "/api/settings/sectioned-inbox").openConnection() as java.net.HttpURLConnection
        c.requestMethod = "PUT"
        c.doOutput = true
        c.setRequestProperty("Content-Type", "application/json")
        c.setRequestProperty("Authorization", "Bearer stub-token")
        c.outputStream.use { it.write("{\"enabled\":$enabled}".toByteArray()) }
        check(c.responseCode == 200) { "the stub did not take the setting: ${c.responseCode}" }
    }
}
