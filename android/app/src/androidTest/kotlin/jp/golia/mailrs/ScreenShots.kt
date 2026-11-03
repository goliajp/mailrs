package jp.golia.mailrs

import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Pictures of the screens the filing work changed, in each language and
 * both themes, for a person to look at. Not a check: it runs only when
 * asked (`MAILRS_SHOTS=1 scripts/android-build.sh test <class>`, which
 * passes `shots=true`) and saves PNGs to `/data/local/tmp/mailrs-shots`
 * on the device, from where `adb pull` takes them.
 */
abstract class ScreenShots(private val tag: String, private val theme: String) : MailrsUiTest() {
    init {
        configuration.appLocale = tag
        configuration.night = when (theme) {
            "dark" -> "yes"
            else -> "no"
        }
    }

    /**
     * `screencap` by the shell, into `/data/local/tmp/mailrs-shots`: the
     * app's own files go when the test run uninstalls it, and the shell's
     * directory is where `adb pull` reads from.
     */
    private fun shot(name: String) {
        compose.waitForIdle()
        val ui = InstrumentationRegistry.getInstrumentation().uiAutomation
        for (command in listOf("mkdir -p $DIR", "screencap -p $DIR/$tag-$theme-$name.png")) {
            android.os.ParcelFileDescriptor.AutoCloseInputStream(ui.executeShellCommand(command)).use { it.readBytes() }
        }
    }

    private companion object {
        const val DIR = "/data/local/tmp/mailrs-shots"
    }

    @Test
    fun the_changed_screens() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("shots") == "true")
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        shot("1-inbox")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        // the drawer's slide-in: the test owns the frame clock, so it is advanced, not waited for
        compose.mainClock.advanceTimeBy(600)
        shot("2-drawer")
        compose.onNodeWithTag("drawer.item.Inbox").performScrollTo().performClick()
        waitForTag("row.conversation", "the inbox did not come back")
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput { longClick() }
        waitForTag("button.selectionMore", "selecting did not offer more actions")
        compose.onNodeWithTag("button.selectionMore").performClick()
        waitForTag("action.Trash", "the menu did not open")
        shot("3-selection-menu")
        // back closes the menu, and may end the selection with it
        pressBack()
        if (compose.onAllNodesWithTag("button.endSelection").fetchSemanticsNodes().isNotEmpty()) {
            compose.onNodeWithTag("button.endSelection").performClick()
        }
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("button.threadMore", "the thread did not open")
        compose.onNodeWithTag("button.threadMore").performClick()
        waitForTag("action.Pin", "the thread menu did not open")
        shot("4-thread-menu")
    }
}

@RunWith(AndroidJUnit4::class) class ShotsEnLight : ScreenShots("en", "light")
@RunWith(AndroidJUnit4::class) class ShotsEnDark : ScreenShots("en", "dark")
@RunWith(AndroidJUnit4::class) class ShotsJaLight : ScreenShots("ja", "light")
@RunWith(AndroidJUnit4::class) class ShotsJaDark : ScreenShots("ja", "dark")
@RunWith(AndroidJUnit4::class) class ShotsZhLight : ScreenShots("zh-CN", "light")
@RunWith(AndroidJUnit4::class) class ShotsZhDark : ScreenShots("zh-CN", "dark")
