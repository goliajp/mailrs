package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Drafts: kept, reopened, and discarded.
 *
 * Its own file rather than a corner of the composer's — writing a
 * message and managing the ones you did not send are different
 * questions, and this one is about what happens when the server
 * refuses.
 */
@RunWith(AndroidJUnit4::class)
class DraftsFlowTest : MailrsUiTest() {

    /**
     * A draft that could not be discarded comes back and says why.
     *
     * The row goes optimistically, which is right — a discard that
     * waits for the server feels broken — but the answer was being
     * dropped entirely: `client.deleteDraft` was launched and its
     * outcome never read. So a refusal showed a draft discarded and
     * left it on the server, to reappear the next time the list was
     * opened with no explanation of where it had been.
     */
    @Test
    fun a_draft_that_cannot_be_discarded_comes_back() {
        java.net.URL(stubBase() + "/debug/refuse-verb/discard").openConnection()
            .let { it as java.net.HttpURLConnection }
            .apply { requestMethod = "POST" }
            .inputStream.use { it.readBytes() }

        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        // There has to be a draft to discard, and the way one exists is
        // to start a message and leave it.
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.subject", "the composer never opened")
        compose.onNodeWithTag("field.subject").performTextInput("Not going anywhere")
        compose.onNodeWithTag("button.cancel").performClick()
        waitForTag("list.conversations", "the composer never closed")

        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Drafts").performClick()
        waitForTag("list.drafts", "the drafts list never opened")

        val before = compose.onAllNodesWithTag("row.draft").fetchSemanticsNodes().size
        assertTrue("nothing to discard", before > 0)
        compose.onAllNodesWithTag("button.discardDraft").onFirst().performClick()

        // Back on the list, because it was not discarded.
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.draft").fetchSemanticsNodes().size == before
        }
    }
}
