package jp.golia.mailrs

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
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

    /**
     * A keyboard can start a message and a search.
     *
     * The two-pane layout exists because this app runs on tablets and
     * opened foldables, and those are the devices that come with a
     * keyboard attached. Every mail client worth using answers `c` and
     * `/` there; without them the keyboard is decoration, and reaching
     * for the screen is the only way to do the two things people do
     * most.
     */
    @Test
    fun the_keyboard_starts_a_message_and_a_search() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.onNodeWithTag("list.conversations").performKeyInput { pressKey(Key.C) }
        waitForTag("field.to", "the c key did not start a message")
        pressEscape()
        waitForTag("list.conversations", "escape did not close the composer")

        compose.onNodeWithTag("list.conversations").performKeyInput { pressKey(Key.Slash) }
        waitForTag("search.field", "the slash key did not open the search")

        // And the key that closes things. Without it the keyboard can
        // open two screens and leave neither — which is worse than not
        // opening them, because the hand is already off the screen.
        pressEscape()
        waitForTag("list.conversations", "escape did not close the search")
    }

    /**
     * A key from the window, not from a node.
     *
     * `performKeyInput` needs the node it is called on to hold focus,
     * and a composer that has just opened holds none — so the key went
     * nowhere and read as "escape did nothing". Dispatching to the
     * activity is what a keyboard actually does.
     */
    private fun pressEscape() {
        compose.activityRule.scenario.onActivity { activity ->
            val down = android.view.KeyEvent(
                android.view.KeyEvent.ACTION_DOWN,
                android.view.KeyEvent.KEYCODE_ESCAPE,
            )
            val up = android.view.KeyEvent(
                android.view.KeyEvent.ACTION_UP,
                android.view.KeyEvent.KEYCODE_ESCAPE,
            )
            activity.dispatchKeyEvent(down)
            activity.dispatchKeyEvent(up)
        }
        compose.waitForIdle()
    }

    /**
     * Beside the list, the message has no way back.
     *
     * There is nowhere to go: the list is already on screen. Android's
     * list-detail guidance says the detail pane carries no back
     * affordance when both are showing, and an arrow pointing at
     * something already visible is a control whose meaning has to be
     * guessed. The phone layout keeps its arrow — `back_closes_the_
     * thread_rather_than_the_app` is that half.
     */
    @Test
    fun the_message_beside_the_list_offers_no_way_back() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the message did not open")

        compose.onAllNodesWithTag("button.back").assertCountEquals(0)
    }
}
