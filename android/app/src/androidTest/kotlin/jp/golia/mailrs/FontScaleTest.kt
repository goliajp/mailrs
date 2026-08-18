package jp.golia.mailrs

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The app at twice the system font size.
 *
 * Android's accessibility settings go to 200%, a mail client is nothing
 * but text, and this is the configuration most likely to be turned on
 * for real — by somebody who needs it. The whole class runs enlarged
 * rather than one test switching mid-run, because the switch itself
 * recreates the activity.
 *
 * What it looks for is a row whose height was fixed in dp swallowing
 * text that grew in sp: the list still lists, a message still opens,
 * and the way back is still there to be pressed.
 */
@RunWith(AndroidJUnit4::class)
class FontScaleTest : MailrsUiTest() {

    init {
        configuration.fontScale = "2.0"
    }

    @Test
    fun the_inbox_reads_and_a_message_opens() {
        // The witness. Without it this test passes whether or not the
        // rule ever applied — a measurement device that has failed
        // looks exactly like data, and "everything worked at 200%" is
        // the reading a scale of 1.0 would also give.
        assertEquals(
            2.0f,
            compose.activity.resources.configuration.fontScale,
            0.01f,
        )

        signIn()
        waitForTag("list.conversations", "the inbox never listed at 200% text")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the message did not open at 200% text")
        compose.onNodeWithTag("button.back").assertIsDisplayed()
    }
}
