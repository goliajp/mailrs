package jp.golia.mailrs

import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The app in the language the phone gives it. The words sampled are the
 * ones every visit passes — the list's title, the drawer, the menu and
 * the undo — and the English for the same places must not appear.
 */
abstract class LocalizedScreens(private val tag: String, private val words: Map<String, String>) : MailrsUiTest() {
    init {
        configuration.appLocale = tag
    }

    private fun present(text: String) =
        compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun sees(key: String) {
        val text = words.getValue(key)
        runCatching { compose.waitUntil(TIMEOUT_MS) { present(text) } }
            .onFailure { throw AssertionError("[$tag] expected \"$text\" for $key") }
    }

    @Test
    fun the_list_the_drawer_and_the_menu_speak_the_language() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        sees("inbox")
        assertTrue("[$tag] the title is still English", !present("Inbox"))

        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        sees("trash")
        sees("starred")
        assertTrue("[$tag] the drawer is still English", !present("Starred"))
        // closed by choosing the inbox again; back leaves the app from the list
        compose.onNodeWithTag("drawer.item.Inbox").performScrollTo().performClick()
        waitForTag("row.conversation", "the inbox did not come back")

        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput { longClick() }
        waitForTag("button.selectionMore", "selecting did not offer more actions")
        compose.onNodeWithTag("button.selectionMore").performClick()
        sees("toTrash")
        compose.onNodeWithTag("action.Trash").performClick()
        sees("undo")
        assertTrue("[$tag] the undo is still English", !present("Undo"))
    }
}

@RunWith(AndroidJUnit4::class)
class JapaneseScreensTest : LocalizedScreens(
    "ja",
    mapOf("inbox" to "受信トレイ", "trash" to "ゴミ箱", "starred" to "スター付き", "toTrash" to "ゴミ箱に移動", "undo" to "元に戻す"),
)

@RunWith(AndroidJUnit4::class)
class ChineseScreensTest : LocalizedScreens(
    "zh-CN",
    mapOf("inbox" to "收件箱", "trash" to "废纸篓", "starred" to "已加星", "toTrash" to "移到废纸篓", "undo" to "撤销"),
)
