package jp.golia.mailrs

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * A tablet, or a foldable that has been opened.
 *
 * The display is made wide **before the activity launches**, not
 * during the test. Resizing underneath a running app hung the suite
 * twice for want of a frame that never came — the app was idle, the
 * main thread parked in its looper, and Compose waited for a
 * Choreographer callback the reconfigured display never delivered. A
 * configuration the app is born into has nothing to survive.
 */
@RunWith(AndroidJUnit4::class)
class WideScreenTest : MailrsUiTest() {

    init {
        // 1600 × 2560 at 240dpi is 1067 × 1707dp — a tablet, and well
        // past the 840dp where the second pane appears.
        configuration.displaySize = "1600x2560"
        configuration.density = "240"
    }

    @Test
    fun the_list_and_the_message_are_shown_together() {
        // The witness: at phone width this test would pass its own
        // assertions for the wrong reason, by never reaching them.
        assertTrue(
            "the display was not made wide, so this proves nothing",
            compose.activity.resources.configuration.screenWidthDp >= 840,
        )

        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        // Both panes, and the empty one saying so rather than being
        // blank — a pane with nothing in it and no explanation looks
        // like something failed to load.
        waitForTag("pane.detail", "there was no second pane")
        compose.onNodeWithText("No conversation open").assertIsDisplayed()

        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the message did not open")
        // The list is still there. On a phone it would have gone.
        compose.onNodeWithTag("list.conversations").assertIsDisplayed()
    }
}
