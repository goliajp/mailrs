package jp.golia.mailrs

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import jp.golia.mailrs.wire.ServerStore
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The sign-in screen chooses its server from a saved list rather than
 * taking one typed by hand.
 *
 * None of these point the app at the stub through the test hook: the
 * server a sign-in reaches is the one chosen from the list, and with
 * the hook in place every choice would reach the stub and prove nothing.
 */
@RunWith(AndroidJUnit4::class)
class ServerPickerFlowTest : MailrsUiTest() {

    @Before
    fun startWithAFreshList() {
        compose.activityRule.scenario.onActivity { it.resetServersForTest() }
        compose.waitForIdle()
    }

    private fun present(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun serverFieldSays(): String =
        compose.onNodeWithTag("field.server").fetchSemanticsNode()
            .config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString().orEmpty()

    private fun openServers() {
        compose.onNodeWithTag("field.server").performClick()
        waitForTag("sheet.servers", "the server list never opened")
    }

    private fun addServer(name: String, address: String) {
        compose.onNodeWithTag("server.add").performClick()
        waitForTag("server.field.address", "the add form never opened")
        compose.onNodeWithTag("server.field.name").performTextInput(name)
        compose.onNodeWithTag("server.field.address").performTextInput(address)
        compose.onNodeWithTag("server.save").performClick()
        waitForTag("server.row.$name", "the list never showed $name")
    }

    private fun waitUntilGone(tag: String, what: String) {
        runCatching { compose.waitUntil(TIMEOUT_MS) { !present(tag) } }
        assertTrue(what, !present(tag))
    }

    @Test
    fun a_fresh_list_offers_the_built_in_server() {
        assertTrue(serverFieldSays(), serverFieldSays().contains("GoliaMail"))
        assertTrue(serverFieldSays(), serverFieldSays().contains("mail.golia.ai"))
        openServers()
        compose.onNodeWithTag("server.row.GoliaMail").assertIsSelected()
        // the only entry cannot be hidden
        assertTrue("the only server offered to hide itself", !present("server.hideBuiltIn"))
    }

    @Test
    fun an_added_server_is_chosen_signed_in_to_and_remembered() {
        openServers()
        addServer("Stub", stubBase())
        compose.onNodeWithTag("server.row.Stub").assertIsSelected()
        compose.onNodeWithTag("server.row.Stub").performClick()
        waitUntilGone("sheet.servers", "choosing a server left the list open")
        assertTrue(serverFieldSays(), serverFieldSays().contains("Stub"))

        compose.onNodeWithTag("field.address").performTextInput("me@golia.jp")
        compose.onNodeWithTag("field.password").performTextInput("anything")
        compose.onNodeWithTag("button.signIn").performClick()
        // no test hook points at the stub here, so a list is the chosen server answering
        waitForTag("list.conversations", "the chosen server never listed the inbox")

        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val stored = ServerStore(context) { null }.list.value
        assertEquals("the server that signed in was not remembered", "Stub", stored.preselected.name)

        compose.activityRule.scenario.onActivity { it.signOutForTest() }
        waitForTag("field.server", "the sign-in screen never came back")
        assertTrue(serverFieldSays(), serverFieldSays().contains("Stub"))
    }

    @Test
    fun a_bad_or_repeated_address_cannot_be_saved() {
        openServers()
        compose.onNodeWithTag("server.add").performClick()
        waitForTag("server.field.address", "the add form never opened")
        compose.onNodeWithTag("server.save").assertIsNotEnabled()

        compose.onNodeWithTag("server.field.address").performTextInput("not a server")
        compose.onNodeWithTag("server.addressNote", useUnmergedTree = true)
            .assertTextContains("Enter a server address, like mail.example.com")
        compose.onNodeWithTag("server.save").assertIsNotEnabled()

        compose.onNodeWithTag("server.field.address").performTextReplacement("https://mail.golia.ai/")
        compose.onNodeWithTag("server.addressNote", useUnmergedTree = true)
            .assertTextContains("This server is already in the list")
        compose.onNodeWithTag("server.save").assertIsNotEnabled()

        compose.onNodeWithTag("server.field.address").performTextReplacement("mail.example.com")
        compose.onNodeWithTag("server.save").assertIsEnabled()
        compose.onNodeWithTag("server.cancel").performClick()
        waitUntilGone("server.field.address", "cancel left the form open")
        assertTrue("a cancelled server was saved", !present("server.row.mail.example.com"))
    }

    @Test
    fun an_added_server_is_renamed_and_deleted() {
        openServers()
        addServer("Office", "mail.example.com")

        compose.onNodeWithTag("server.edit.Office").performClick()
        waitForTag("server.field.name", "the edit form never opened")
        compose.onNodeWithTag("server.field.name").assertTextContains("Office")
        compose.onNodeWithTag("server.field.address").assertTextContains("mail.example.com")
        compose.onNodeWithTag("server.field.name").performTextReplacement("Work")
        compose.onNodeWithTag("server.save").performClick()
        waitForTag("server.row.Work", "the list never showed the new name")
        assertTrue("the old name stayed", !present("server.row.Office"))
        // it was the chosen one, and a rename does not change that
        compose.onNodeWithTag("server.row.Work").assertIsSelected()

        compose.onNodeWithTag("server.edit.Work").performClick()
        waitForTag("server.delete", "the edit form has no delete")
        compose.onNodeWithTag("server.delete").performClick()
        waitUntilGone("server.row.Work", "the deleted server stayed in the list")
        // the choice falls back to a server that is still listed
        compose.onNodeWithTag("server.row.GoliaMail").assertIsSelected()
    }

    @Test
    fun the_built_in_server_hides_and_returns() {
        openServers()
        addServer("Office", "mail.example.com")

        compose.onNodeWithTag("server.hideBuiltIn").performClick()
        waitUntilGone("server.row.GoliaMail", "the hidden server stayed in the list")
        waitForTag("server.restore", "nothing offers the hidden server back")

        compose.onNodeWithTag("server.restore").performClick()
        waitForTag("server.row.GoliaMail", "the built-in server did not come back")
        waitUntilGone("server.restore", "the offer stayed after it was taken")
    }
}
