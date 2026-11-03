package jp.golia.mailrs

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
import java.io.File

/**
 * Pictures of the screens the filing work changed, in each language and
 * both themes, for a person to look at. Not a check: it runs only when
 * asked (`-e shots true`) and saves PNGs under the app's files dir, from
 * where they are pulled with `adb pull`.
 */
abstract class ScreenShots(private val tag: String, private val theme: String) : MailrsUiTest() {
    init {
        configuration.appLocale = tag
        configuration.night = when (theme) {
            "dark" -> "yes"
            else -> "no"
        }
    }

    private fun shot(name: String) {
        compose.waitForIdle()
        val bitmap = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        val dir = File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir, "shots")
        dir.mkdirs()
        File(dir, "$tag-$theme-$name.png").outputStream().use {
            bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)
        }
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
        pressBack()
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput { longClick() }
        waitForTag("button.selectionMore", "selecting did not offer more actions")
        compose.onNodeWithTag("button.selectionMore").performClick()
        waitForTag("action.Trash", "the menu did not open")
        shot("3-selection-menu")
        pressBack()
        pressBack()
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
