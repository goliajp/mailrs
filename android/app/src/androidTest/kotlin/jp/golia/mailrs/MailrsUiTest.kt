package jp.golia.mailrs

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.printToString
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeRight
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What every UI test in this app needs: a launched activity, a stub to
 * talk to, and the handful of waits that make a Compose assertion mean
 * something.
 *
 * The helpers are here rather than copied because each of them exists
 * for a defect that has already happened once — waiting for *displayed*
 * rather than *present*, waiting for a row to stop moving before
 * tapping it, waiting for sign-in to produce an answer of either kind.
 * A second copy is a second place for one of those lessons to be
 * missing.
 */
abstract class MailrsUiTest : GrantsNotifications() {


    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    /**
     * Every test starts signed out.
     *
     * The token store survives the process, so without this the second
     * run of the suite opens on the inbox and the sign-in tests fail
     * looking for fields that are not there — a stale session wearing
     * the costume of a broken locator.
     */
    /**
     * The stub keeps what it was sent until told otherwise, so a run
     * that asserts on `/debug/sent` can be reading the previous run's
     * message. The iOS suite resets it in `launch()` for the same
     * reason; skipping it is how a passing assertion stops meaning
     * anything.
     */
    @Before
    fun startSignedOut() {
        compose.activityRule.scenario.onActivity { it.signOutForTest() }
        // Displayed, not merely present. Signing out slides the sign-in
        // screen back in, and a field that exists but is still off the
        // left edge takes a click at a coordinate that is not on it.
        // One run in fourteen failed here, filled fields and all.
        waitForTag("field.address", "the sign-in screen never came back")
    }

    /** The activity is launched by the rule; point it at the stub. */
    protected fun signIn(address: String = "me@golia.jp", password: String = "anything") {
        val stub = InstrumentationRegistry.getArguments().getString("mailrsBaseURL")
            ?: DEFAULT_STUB
        compose.activityRule.scenario.onActivity { it.useStubServer(stub) }

        compose.onNodeWithTag("field.address").performTextInput(address)
        compose.onNodeWithTag("field.password").performTextInput(password)
        compose.onNodeWithTag("button.signIn").performClick()

        // Wait for an answer of either kind rather than for the inbox.
        // "the inbox never listed" is what a failed sign-in looks like
        // from the caller, and it names neither the error the server
        // gave nor the fact that the tap never landed — both of which
        // have happened here, intermittently, and neither of which the
        // old message could tell apart.
        try {
            compose.waitUntil(TIMEOUT_MS) {
                compose.onAllNodes(hasTestTag("list.conversations")).fetchSemanticsNodes().isNotEmpty() ||
                    compose.onAllNodes(hasTestTag("text.signInError")).fetchSemanticsNodes().isNotEmpty()
            }
        } catch (e: Throwable) {
            throw AssertionError(
                "sign-in produced neither a list nor an error. still on sign-in: " +
                    compose.onAllNodes(hasTestTag("button.signIn")).fetchSemanticsNodes().isNotEmpty(),
                e,
            )
        }
        val failed = compose.onAllNodes(hasTestTag("text.signInError")).fetchSemanticsNodes()
        if (failed.isNotEmpty()) {
            throw AssertionError(
                "the server refused the sign-in: " +
                    failed.first().config.getOrNull(SemanticsProperties.Text)?.joinToString(),
            )
        }
    }

    /**
     * Compose's idling waits for recomposition, not for a network call
     * on `Dispatchers.IO` — so a bare assertion after a click races the
     * response. Every wait here is bounded and says what it was waiting
     * for, because "test timed out" names nothing.
     */
    /**
     * Wait until something is **on screen**, not until it exists.
     *
     * These were two different checks: it waited for the node to appear
     * and then asserted, once, that it was displayed. A node that exists
     * but is still sliding or expanding fails that single assertion, so
     * the helper was sound only as long as nothing animated. Adding
     * screen transitions turned two search tests red without touching
     * search — the failure was in the waiting, not the app.
     */
    protected fun waitForTag(tag: String, what: String) {
        try {
            compose.waitUntil(TIMEOUT_MS) {
                compose.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty() &&
                    runCatching { compose.onAllNodesWithTag(tag).onFirst().assertIsDisplayed() }.isSuccess
            }
        } catch (e: Throwable) {
            throw AssertionError(
                what + "\n" + compose.onRoot().printToString(maxDepth = 12).take(2500),
                e,
            )
        }
    }

    protected fun tapWhenSteady(tag: String, index: Int) {
        compose.onAllNodesWithTag(tag)[index].performScrollTo()
        var last = Float.NaN
        compose.waitUntil(TIMEOUT_MS) {
            val top = compose.onAllNodesWithTag(tag)[index].fetchSemanticsNode().positionInRoot.y
            val steady = top == last
            last = top
            steady
        }
        compose.onAllNodesWithTag(tag)[index].performClick()
    }

    protected fun pressBack() {
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
    }




    /** And back leaves the selection rather than the app. */







    /** A DMARC row is passing against total, which is what a report is for. */



    /** A key is told apart by its prefix and its scopes, which is what a revoke needs. */


    /** Permission groups say which are built in, which is the first thing to know. */




    /** Apps say who owns them and what they may do. */




    /** The launcher's Search shortcut opens the search, not the inbox. */




    /** Where the stub is, for the tests that poke its debug routes. */
    protected fun stubBase(): String =
        InstrumentationRegistry.getArguments().getString("mailrsBaseURL") ?: DEFAULT_STUB

    protected fun readStub(path: String): String {
        val stub = InstrumentationRegistry.getArguments().getString("mailrsBaseURL") ?: DEFAULT_STUB
        return java.net.URL(stub + path).openStream().bufferedReader().use { it.readText() }
    }

    protected companion object {
            /**
         * Guest-local, reached through `adb reverse` — see
         * `scripts/android-build.sh`. Not `10.0.2.2`: that crosses the
         * emulator's NAT, and a suite's worth of short-lived
         * connections through it stalls a connect every so often, which
         * arrives as one unrelated test failing per run.
         */
        const val DEFAULT_STUB = "http://127.0.0.1:6039"
        const val TIMEOUT_MS = 15_000L
    }
}

