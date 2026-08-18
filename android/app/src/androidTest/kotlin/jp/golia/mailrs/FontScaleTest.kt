package jp.golia.mailrs

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
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

    /**
     * The composer is the densest form here — four labelled lines above
     * the message — and the one where text growing past its row would
     * cost somebody the Send button.
     */
    @Test
    fun a_message_can_be_written_and_sent() {
        signIn()
        waitForTag("button.compose", "the compose button never appeared at 200% text")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened at 200% text")
        compose.onNodeWithTag("field.to").performTextInput("someone@golia.jp")
        compose.onNodeWithTag("field.subject").performTextInput("Large type")
        compose.onNodeWithTag("field.body").performTextInput("Readable.")
        compose.onNodeWithTag("button.send").assertIsDisplayed().performClick()
        waitForTag("list.conversations", "sending never came back to the list at 200% text")
    }

    /**
     * The message itself grows too.
     *
     * A mail body is HTML in a WebView, and a WebView ignores the
     * system font scale — its text zoom is 100 until something says
     * otherwise. So at 200% the app's own chrome doubled and the words
     * somebody actually came to read stayed exactly as small as
     * before, which is the one part that mattered.
     */
    @Test
    fun the_message_body_grows_with_the_system_text_size() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed at 200% text")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the message did not open at 200% text")

        val zoom = webViewTextZoom()
        assertEquals("the mail body ignored the system text size", 200, zoom)
    }

    /** The text zoom of the one WebView on screen. */
    private fun webViewTextZoom(): Int {
        var found: Int? = null
        compose.activityRule.scenario.onActivity { activity ->
            found = firstWebView(activity.window.decorView)?.settings?.textZoom
        }
        return found ?: error("no message body was on screen to measure")
    }

    private fun firstWebView(view: android.view.View): android.webkit.WebView? {
        if (view is android.webkit.WebView) return view
        if (view !is android.view.ViewGroup) return null
        for (i in 0 until view.childCount) {
            firstWebView(view.getChildAt(i))?.let { return it }
        }
        return null
    }
}
