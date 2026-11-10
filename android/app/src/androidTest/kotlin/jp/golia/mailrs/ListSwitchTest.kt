package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What the screen shows between choosing a list and its first page
 * arriving: placeholder rows from the top, never the rows saved the last
 * time that list was open, which were swapped for the fresh ones in front
 * of the reader.
 */
@RunWith(AndroidJUnit4::class)
class ListSwitchTest : MailrsUiTest() {

    @After
    fun noDelay() = setDelay(0)

    @Test
    fun switching_back_shows_placeholders_not_the_saved_rows() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        shot("1-inbox")
        open("drawer.item.Starred")
        waitForText("Starred thread", "starred never listed")

        setDelay(2500)
        open("drawer.item.Inbox")
        waitForTag("list.loading", "no placeholder rows while the inbox was loading")
        shot("2-inbox-loading")
        assertTrue(
            "the inbox's saved rows were drawn before it answered",
            compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().isEmpty(),
        )
        waitForTag("list.conversations", "the inbox never came back")
        assertTrue(
            "the placeholder outlived the load",
            compose.onAllNodesWithTag("list.loading").fetchSemanticsNodes().isEmpty(),
        )
        shot("3-inbox")
    }

    private fun open(item: String) {
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag(item).performScrollTo().performClick()
    }

    private fun waitForText(text: String, what: String) {
        runCatching {
            compose.waitUntil(TIMEOUT_MS) { compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
        }.onFailure { throw AssertionError(what) }
    }

    private fun setDelay(ms: Int) {
        val c = java.net.URL(stubBase() + "/debug/set-delay").openConnection() as java.net.HttpURLConnection
        c.requestMethod = "POST"
        c.doOutput = true
        c.setRequestProperty("Content-Type", "application/json")
        c.outputStream.use { it.write("{\"ms\":$ms}".toByteArray()) }
        c.inputStream.use { it.readBytes() }
        c.disconnect()
    }

    /** Pictures for a person, only when the run asks (`shots=true`). */
    private fun shot(name: String) {
        if (InstrumentationRegistry.getArguments().getString("shots") != "true") return
        val ui = InstrumentationRegistry.getInstrumentation().uiAutomation
        val dir = "/data/local/tmp/mailrs-shots"
        for (command in listOf("mkdir -p $dir", "screencap -p $dir/list-switch-$name.png")) {
            android.os.ParcelFileDescriptor.AutoCloseInputStream(ui.executeShellCommand(command)).use { it.readBytes() }
        }
    }
}
