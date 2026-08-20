package jp.golia.mailrs

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The invitation, on screen.
 *
 * Everything under this was proven separately — the server resolves the
 * instant, the thread response marks the message, the payload decodes —
 * and none of it says the card is *mounted*. A card that renders
 * nothing looks exactly like a message carrying no calendar part, which
 * is how the iOS one shipped blank through five green assertions and a
 * whole test suite. This is what tells them apart.
 */
@RunWith(AndroidJUnit4::class)
class InviteFlowTest : MailrsUiTest() {
    @Test
    fun an_invitation_renders_its_card() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")
        waitForTag("invite.card", "the card did not mount")

        compose.onNodeWithTag("invite.card").performScrollTo()
        compose.onNodeWithTag("invite.summary").assertIsDisplayed()

        // Exchange does not send METHOD:UPDATE — it re-sends with a
        // higher SEQUENCE — so a re-sent meeting must not read as a
        // first invitation.
        compose.onNodeWithText("Updated invite").assertIsDisplayed()

        // The whole timezone argument in one line: 16:00 in Santa Clara
        // is 08:00 the next morning here, and the organiser's own clock
        // is named beside it because neither number alone is the
        // answer.
        compose.onNodeWithText("Pacific Standard Time", substring = true).assertIsDisplayed()
        compose.onNodeWithText("8:00", substring = true).assertIsDisplayed()

        // The two things a reader does with a meeting.
        compose.onNodeWithTag("invite.join").assertIsDisplayed()
        compose.onNodeWithTag("invite.accepted").assertIsDisplayed()
    }
}
