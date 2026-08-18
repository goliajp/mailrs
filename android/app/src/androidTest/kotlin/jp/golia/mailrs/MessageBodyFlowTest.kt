package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * How a message's own HTML is rendered.
 *
 * Its own file: mail is authored elsewhere, for other clients and
 * other screens, and what this app does with what arrives — widths,
 * remote content, colours — is a subject of its own.
 */
@RunWith(AndroidJUnit4::class)
class MessageBodyFlowTest : MailrsUiTest() {

    /**
     * A message authored at 760px fits the phone.
     *
     * Newsletters are laid out for a desktop — a fixed-width table with
     * a fixed-width `div` inside it — and the stylesheet this app
     * injects constrained tables and images but not the div, so the
     * text ran off the right edge and the last word of every line was
     * cut in half. Visible at any size and unmissable at 200%, where
     * looking at a screenshot is what found it.
     *
     * The assertion is the WebView's own content width against the
     * screen: horizontal scrolling in a mail body is the defect.
     */
    @Test
    fun a_message_laid_out_for_a_desktop_fits_the_phone() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")
        // The body renders asynchronously; its width settles after.
        Thread.sleep(1_500)

        var content = 0
        var visible = 0
        compose.activityRule.scenario.onActivity { activity ->
            val web = firstWebView(activity.window.decorView) ?: return@onActivity
            content = (web.contentHeight * 0) + web.computeHorizontalScrollRangeCompat()
            visible = web.width
        }
        assertTrue("no message body was on screen", visible > 0)
        assertTrue(
            "the mail is $content px wide in a $visible px window, so it scrolls sideways",
            content <= visible + 2,
        )
    }

    private fun android.webkit.WebView.computeHorizontalScrollRangeCompat(): Int {
        val m = android.view.View::class.java.getDeclaredMethod("computeHorizontalScrollRange")
        m.isAccessible = true
        return m.invoke(this) as Int
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
