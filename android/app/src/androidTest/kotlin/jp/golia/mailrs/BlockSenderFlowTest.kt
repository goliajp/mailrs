package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Blocking a sender from their message: the menu names the address, the
 * question says where the mail goes, the block and the conversation's move
 * reach the server as one call, and the snackbar's Undo takes it back. The
 * same journey as the iPhone's `BlockSenderFlowTests`.
 */
@RunWith(AndroidJUnit4::class)
class BlockSenderFlowTest : MailrsUiTest() {

    private fun verbsContain(entry: String, what: String) {
        var verbs = ""
        runCatching {
            compose.waitUntil(TIMEOUT_MS) {
                verbs = readStub("/debug/verbs")
                verbs.contains(entry)
            }
        }.onFailure { throw AssertionError("$what; the stub saw $verbs") }
    }

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    /** Alice's message's own menu, opened, with the sender lists read. */
    private fun openAlicesMenu() {
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        unfoldAll()
        compose.onNodeWithTag("message.settings.1").performScrollTo().performClick()
        runCatching { compose.waitUntil(TIMEOUT_MS) { exists("sender.block") || exists("sender.unblock") } }
    }

    @Test
    fun a_sender_is_blocked_from_their_message_and_the_undo_takes_it_back() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        openAlicesMenu()
        assertTrue("the menu offered no Block", exists("sender.block"))
        compose.onNodeWithText("Block alice@example.com").assertExists()
        compose.onNodeWithTag("sender.block").performClick()

        waitForTag("button.blockSender", "blocking did not ask first")
        assertFalse("blocked before the answer", readStub("/debug/verbs").contains("block-sender"))
        compose.onNodeWithText("Mail from this address will go to Junk", substring = true).assertExists()
        compose.onNodeWithTag("button.blockSender").performClick()

        verbsContain("block-sender t1", "the block never reached the server")
        waitForTag("list.conversations", "the thread did not close once it was in Junk")
        runCatching {
            compose.waitUntil(TIMEOUT_MS) { compose.onAllNodesWithText("Undo").fetchSemanticsNodes().isNotEmpty() }
        }
        compose.onNodeWithText("Blocked alice@example.com").assertExists()
        compose.onNodeWithText("Undo").performClick()
        verbsContain("unblock-sender t1", "the undo never reached the server")

        // unblocked again: the menu offers Block, not Unblock
        openAlicesMenu()
        assertTrue("the undo left the sender blocked", exists("sender.block"))
    }
}
