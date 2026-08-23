package jp.golia.mailrs

import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The merged list of mail from connected mailboxes.
 *
 * No mailbox is connected in a test run and no IMAP stub exists, so
 * what is checked is what a person meets first: that the row opens
 * something, that the something says what to do rather than showing an
 * empty screen, and that back returns to Settings rather than leaving
 * the app.
 *
 * That last one is not incidental. `closeMailAccounts` sat in this app
 * called by nothing, and back from a Settings sub-screen closed the
 * whole of Settings — two screens away from where a person was.
 */
@RunWith(AndroidJUnit4::class)
class MergedMailFlowTest : MailrsUiTest() {

    private fun openMergedMail() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        scrollToTag("settings.mergedMail", "settings never listed the other-mail row")
        compose.onNodeWithTag("settings.mergedMail").performClick()
    }

    /**
     * With nothing connected the screen says where to go, rather than
     * showing an empty list that reads as "your mail is gone".
     */
    @Test
    fun the_row_opens_a_screen_that_says_what_to_do() {
        openMergedMail()
        waitForTag("mail.empty", "the other-mail screen never opened")
    }

    /** Back returns to Settings, not out of it. */
    @Test
    fun back_returns_to_settings() {
        openMergedMail()
        waitForTag("mail.empty", "the other-mail screen never opened")
        pressBack()
        waitForTag("settings.mergedMail", "back left settings altogether")
    }
}
