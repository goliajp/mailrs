package jp.golia.mailrs

import android.app.Instrumentation
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import android.webkit.WebResourceRequest
import android.webkit.WebView
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Why a message was held, and the links and numbers worth a second look.
 *
 * The stub serves the verdict, the sender checks and the link warnings
 * only once armed (`ios/Testing/stub_verdict.py`), so the same thread
 * unarmed is the counter-example: no panel.
 */
@RunWith(AndroidJUnit4::class)
class VerdictFlowTest : MailrsUiTest() {

    /** Arm the suspicious case; the reset before every test disarms it. */
    private fun armVerdicts() {
        val c = java.net.URL(stubBase() + "/debug/verdict").openConnection() as java.net.HttpURLConnection
        c.requestMethod = "POST"
        c.inputStream.use { it.readBytes() }
    }

    private fun openOrdinaryThread() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")
    }

    private fun exists(tag: String, text: String? = null): Boolean {
        var matcher = hasTestTag(tag)
        if (text != null) matcher = matcher and hasText(text, substring = true)
        return compose.onAllNodes(matcher, useUnmergedTree = false).fetchSemanticsNodes().isNotEmpty()
    }

    private fun waitFor(what: String, check: () -> Boolean) {
        runCatching { compose.waitUntil(TIMEOUT_MS) { check() } }
        assertTrue(what, check())
    }

    @Test
    fun a_held_message_shows_its_layers_reasons_and_sender_checks() {
        armVerdicts()
        openOrdinaryThread()
        // the placement note is on the older message
        unfoldAll()
        waitFor("no verdict panel on a held message") { exists("verdict.panel") }
        assertTrue("rule reason not worded", exists("verdict.layer.identity", "The display name claims to be Golia."))
        assertTrue("content rule not listed", exists("verdict.layer.content", "The text asks you to send your own LINE QR code."))
        assertTrue("content rule's score not shown", exists("verdict.layer.content", "+6.0"))
        assertTrue("a passing layer did not say so", exists("verdict.layer.provenance", "Nothing found."))
        assertTrue("no SPF/DKIM/DMARC line", exists("sender.checks", "DKIM: none"))
        // the stub's history says the sender gave 0120-111-222 in another
        // conversation, and +81 3-1234-5678 first in this very message
        waitFor("no callback notice") { exists("callback.notice", "+81 3-1234-5678") }
        assertTrue("a number given in another conversation was named", !exists("callback.notice", "0120-111-222"))
        assertTrue("no placement note on the filed message", exists("verdict.placement", "Promotions"))

        compose.onAllNodesWithTag("verdict.release").onFirst().performScrollTo().performClick()
        var verbs = ""
        waitFor("release never reached the server") {
            verbs = readStub("/debug/verbs")
            verbs.contains("release ")
        }
    }

    /**
     * The iPhone's order on a message that has both: the meeting first,
     * the verdict under it. And the sender line is a name, not the
     * address it was sent from.
     */
    @Test
    fun the_invite_comes_before_the_verdict_and_the_sender_is_a_name() {
        armVerdicts()
        openOrdinaryThread()
        unfoldAll()
        waitFor("no verdict panel on a held message") { exists("verdict.panel") }
        waitFor("no invite card on the invitation") { exists("invite.card") }
        compose.onAllNodesWithTag("invite.card").onFirst().performScrollTo()
        compose.waitForIdle()
        // positions, not bounds: bounds are clipped to the screen, and a
        // panel below the fold reports a top of zero
        val invite = compose.onAllNodesWithTag("invite.card").fetchSemanticsNodes().first().positionInRoot.y
        val verdict = compose.onAllNodesWithTag("verdict.panel").fetchSemanticsNodes().first().positionInRoot.y
        assertTrue("the card was not placed: $invite", invite > 0f)
        assertTrue("the verdict ($verdict) sits above the invite ($invite)", invite < verdict)

        assertTrue("the sender line is not the name", exists("message.header.2", "spoofed"))
        assertTrue("the sender line shows the address", !exists("message.header.2", "spoofed@example.com"))
    }

    @Test
    fun no_verdict_draws_nothing() {
        openOrdinaryThread()
        // the message is there, and nothing was drawn about it
        waitFor("the thread has no message") { exists("button.reply") }
        // bounded: what is waited for here should never come
        runCatching { compose.waitUntil(3_000) { exists("verdict.panel") } }
        assertTrue("a panel was drawn for a message with no verdict", !exists("verdict.panel"))
        assertTrue(!exists("sender.checks"))
        assertTrue(!exists("callback.notice"))
    }

    /** What the body hands the screen when a link in it is tapped. */
    private fun tapInBody(url: String) {
        compose.activityRule.scenario.onActivity { activity ->
            val web = webViews(activity.window.decorView).last()
            val request = object : WebResourceRequest {
                override fun getUrl(): Uri = Uri.parse(url)
                override fun isForMainFrame() = true
                override fun isRedirect() = false
                override fun hasGesture() = true
                override fun getMethod() = "GET"
                override fun getRequestHeaders(): Map<String, String> = emptyMap()
            }
            web.webViewClient.shouldOverrideUrlLoading(web, request)
        }
    }

    @Test
    fun a_flagged_link_asks_first_and_an_ordinary_link_does_not() {
        armVerdicts()
        // the browser is not started; the monitor counts what would have opened
        val filter = IntentFilter(Intent.ACTION_VIEW).apply { addDataScheme("https") }
        val monitor: Instrumentation.ActivityMonitor =
            InstrumentationRegistry.getInstrumentation().addMonitor(filter, null, true)
        try {
            openOrdinaryThread()
            waitFor("no verdict panel") { exists("verdict.panel") }

            tapInBody("mailrs-link-warning:0")
            waitFor("a flagged link opened without asking") { exists("link.question", "webmail-oauth.wzldq.com") }
            assertEquals("it opened before the answer", 0, monitor.hits)
            compose.onAllNodesWithTag("link.cancel").onFirst().performClick()
            waitFor("the question stayed") { !exists("link.question") }
            assertEquals("cancel still opened it", 0, monitor.hits)

            tapInBody("https://example.org/help")
            waitFor("an ordinary link did not open") { monitor.hits == 1 }
            assertTrue("an ordinary link asked first", !exists("link.question"))
        } finally {
            InstrumentationRegistry.getInstrumentation().removeMonitor(monitor)
        }
    }

    private fun webViews(view: android.view.View): List<WebView> {
        if (view is WebView) return listOf(view)
        if (view !is android.view.ViewGroup) return emptyList()
        return (0 until view.childCount).flatMap { webViews(view.getChildAt(it)) }
    }
}
