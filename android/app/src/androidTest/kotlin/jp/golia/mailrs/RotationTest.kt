package jp.golia.mailrs

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import android.app.Activity
import android.content.pm.ActivityInfo
import android.content.res.Configuration
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.runner.lifecycle.ActivityLifecycleCallback
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What a configuration change is allowed to take away.
 *
 * Turning the phone destroys the activity and rebuilds it, and every
 * `remember` in it goes with it. What survives is a decision made per
 * piece of state, so it is worth a file of its own: the view model's
 * fields survive by construction, `rememberSaveable` carries the screen
 * decisions, and the password is deliberately in neither.
 */
@RunWith(AndroidJUnit4::class)
class RotationTest : MailrsUiTest() {

    /**
     * A half-written message survives the phone being turned.
     *
     * The text lives in the view model and was never at risk. What was
     * at risk is everything around it: which extra address lines are
     * showing is a screen decision, and losing it hides a Cc somebody
     * had already typed — the address stays in the draft and disappears
     * from view, which is worse than losing it outright.
     */
    @Test
    fun a_rotation_keeps_the_message_being_written() {
        signIn()
        waitForTag("button.compose", "the compose button never appeared")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("button.ccBcc", "the composer never opened")
        compose.onNodeWithTag("button.ccBcc").performClick()
        compose.onNodeWithTag("field.body").performTextInput("half a thought")

        rotated {
            compose.onNodeWithTag("field.body").assertTextContains("half a thought", substring = true)
            compose.onAllNodesWithTag("button.ccBcc").assertCountEquals(0)
        }
    }

    /**
     * Run a block with the phone on its side, and put it back
     * afterwards — a landscape activity left behind would meet the next
     * test as a layout it has never seen.
     */
    private fun rotated(block: () -> Unit) {
        var start = Configuration.ORIENTATION_PORTRAIT
        compose.activityRule.scenario.onActivity { start = it.resources.configuration.orientation }
        try {
            turn(ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE, Configuration.ORIENTATION_LANDSCAPE)
            block()
        } finally {
            // back to the orientation it started in, by name: asking for
            // "unspecified" never rebuilt the activity, so it stayed
            // landscape until the test ended
            val back = if (start == Configuration.ORIENTATION_PORTRAIT) {
                ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
            } else {
                ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
            }
            turn(back, start)
        }
    }

    /**
     * Ask for an orientation and wait until the activity it rebuilds is
     * resumed, before asking Compose to go idle.
     *
     * `waitForIdle` straight after the request races the relaunch. Its
     * last step picks an attached window, posts a frame callback to it
     * and spins until that callback has run, with no timeout. When it
     * picks the old activity's window just before the relaunch removes
     * it, the callback never runs: the main thread sits idle, the test
     * thread spins, and the test hangs until the runner kills it. Seen
     * in four of thirty repeats of the search test, none of eighty once
     * the wait was here. Waiting for the new activity is a wait on the
     * system, not on the frame clock, so a latch is the right tool.
     */
    private fun turn(request: Int, expected: Int) {
        val monitor = ActivityLifecycleMonitorRegistry.getInstance()
        var before: Activity? = null
        var changes = false
        val rebuilt = CountDownLatch(1)
        val callback = ActivityLifecycleCallback { activity, stage ->
            if (stage == Stage.RESUMED && activity !== before) rebuilt.countDown()
        }
        monitor.addLifecycleCallback(callback)
        try {
            compose.activityRule.scenario.onActivity {
                before = it
                changes = it.resources.configuration.orientation != expected
                it.requestedOrientation = request
            }
            if (changes) {
                assertTrue("turning the phone did not rebuild the activity", rebuilt.await(10, TimeUnit.SECONDS))
            }
        } finally {
            monitor.removeLifecycleCallback(callback)
        }
        compose.waitForIdle()
    }

    /**
     * Turning the phone does not throw away what was on screen.
     *
     * A configuration change destroys and rebuilds the activity, and
     * every `remember` in it. The view model survives — so the search
     * term did — but the flag saying the search was *open* did not, and
     * the composer's revealed Cc line did not either: state and screen
     * disagreed after a rotation.
     */
    @Test
    fun a_rotation_keeps_the_search_open() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.search").performClick()
        waitForTag("search.field", "the search never opened")
        compose.onNodeWithTag("search.field").performTextInput("ref 2026")
        waitForTag("list.searchResults", "the search never returned")

        rotated {
            waitForTag("search.field", "the rotation closed the search")
            compose.onNodeWithTag("search.field").assertTextContains("ref 2026", substring = true)
        }
    }
}
