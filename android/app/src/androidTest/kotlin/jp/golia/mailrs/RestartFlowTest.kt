package jp.golia.mailrs

import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The server restarting during a deploy answers 503 for a few seconds.
 * A read waits as told and goes again once, so a list opened then still
 * opens; a write is never sent twice — archiving or sending twice could do
 * it twice — and says the server is restarting instead.
 */
@RunWith(AndroidJUnit4::class)
class RestartFlowTest : MailrsUiTest() {
    private fun restarting(path: String) {
        java.net.URL(stubBase() + "/debug/restarting/1?path=$path").openConnection()
            .let { it as java.net.HttpURLConnection }
            .apply { requestMethod = "POST" }
            .inputStream.use { it.readBytes() }
    }

    @Test
    fun a_list_opened_during_a_restart_still_opens() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        restarting("/api/mail/sent")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Sent").performClick()
        // The stub's count, not the screen: rows drawn from a cache look
        // the same whether or not the read came back after the 503.
        compose.waitUntil(5_000) {
            readStub("/debug/restart-state").contains("\"served_after\": 1") ||
                readStub("/debug/restart-state").contains("\"served_after\":1")
        }
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("Filed and delivered").fetchSemanticsNodes().isNotEmpty()
        }
        assertFalse(
            "a read was told the server is restarting",
            compose.onAllNodesWithText("restarting", substring = true).fetchSemanticsNodes().isNotEmpty(),
        )
    }

    @Test
    fun a_write_during_a_restart_is_said_and_not_repeated() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        restarting("/api/conversations/batch")
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput { longClick() }
        waitForTag("button.endSelection", "the bar never became an action bar")
        compose.onNodeWithTag("button.selectionArchive").performClick()

        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("restarting", substring = true)
                .fetchSemanticsNodes().isNotEmpty()
        }
        // A second send would have reached the stub, which is no longer
        // restarting, and been recorded. Any repeat happens inside the one
        // request before its answer is shown, so by now it would be there.
        val writes = readStub("/debug/writes")
        assertFalse("the write was sent again: $writes", writes.contains("archive") || writes.contains("batch"))
    }
}
