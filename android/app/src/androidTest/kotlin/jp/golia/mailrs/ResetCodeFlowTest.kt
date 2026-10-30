package jp.golia.mailrs

import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * An account whose password an administrator cleared: signing in opens the
 * reset-code form, saying whom to ask; a wrong code is said as such; the
 * right one sets the new password and the sign-in screen says to use it.
 * The form also opens without a failed sign-in.
 */
@RunWith(AndroidJUnit4::class)
class ResetCodeFlowTest : MailrsUiTest() {
    private fun done(): Boolean = readStub("/debug/reset-state").replace(" ", "").contains("\"done\":true")

    private fun useStub() {
        val stub = InstrumentationRegistry.getArguments().getString("mailrsBaseURL") ?: stubBase()
        compose.activityRule.scenario.onActivity { it.useStubServer(stub) }
    }

    @Test
    fun signing_in_to_a_cleared_account_leads_to_the_code() {
        useStub()
        compose.onNodeWithTag("field.address").performTextInput("cleared@golia.jp")
        compose.onNodeWithTag("field.password").performTextInput("an-old-password")
        compose.onNodeWithTag("button.signIn").performClick()

        waitForTag("text.resetExplain", "the reset form never opened")
        compose.onNodeWithTag("text.resetExplain").assertTextContains("Li Hao (lihao@golia.jp)", substring = true)
        compose.onNodeWithTag("field.resetAddress").assertTextContains("cleared@golia.jp", substring = true)

        compose.onNodeWithTag("field.resetCode").performTextInput("WRONGCODE2")
        compose.onNodeWithTag("field.resetPassword").performTextInput("a sound new passphrase")
        compose.onNodeWithTag("field.resetRepeat").performTextInput("a sound new passphrase")
        compose.onNodeWithTag("button.resetSubmit").performClick()
        waitForTag("text.resetFailure", "a wrong code said nothing")
        compose.onNodeWithTag("text.resetFailure")
            .assertTextContains("Ask Li Hao (lihao@golia.jp) for a new code", substring = true)
        assertTrue("a wrong code went through", !done())

        // typed as handed over: a dash and lower case
        compose.onNodeWithTag("field.resetCode").performTextClearance()
        compose.onNodeWithTag("field.resetCode").performTextInput("goodc-ode23")
        compose.onNodeWithTag("button.resetSubmit").performClick()
        waitForTag("text.resetDone", "the sign-in screen did not say the password is set")
        assertTrue("the reset never reached the server", done())
    }

    @Test
    fun the_code_form_opens_without_a_failed_sign_in() {
        useStub()
        compose.onNodeWithTag("button.haveResetCode").performClick()
        waitForTag("text.resetExplain", "the reset form never opened")
        compose.onNodeWithTag("text.resetExplain").assertTextContains("an administrator", substring = true)
        compose.onNodeWithTag("button.resetCancel").performClick()
        waitForTag("button.signIn", "cancelling did not go back to sign in")
    }
}
