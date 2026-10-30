package jp.golia.mailrs

import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.lifecycle.Lifecycle
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Settings offers the pages the server would answer for this account: its
 * own agent keys and sender lists to anyone, the directory to an
 * administrator, and the pages that show other people's mail traffic — the
 * queue, the audit log, the suppressions — to a super administrator only.
 * The account's rights are read again when the app comes back to the front
 * and after a refusal, so a right taken away hides its page.
 */
@RunWith(AndroidJUnit4::class)
class RoleFlowTest : MailrsUiTest() {
    private fun role(name: String) {
        java.net.URL(stubBase() + "/debug/role/$name").openConnection()
            .let { it as java.net.HttpURLConnection }
            .apply { requestMethod = "POST" }
            .inputStream.use { it.readBytes() }
    }

    private fun meReads(): Int =
        Regex("\"me_reads\":\\s*(\\d+)").find(readStub("/debug/role-state"))?.groupValues?.get(1)?.toInt() ?: -1

    private fun offered(tag: String): Boolean =
        compose.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty()

    private fun openSettings() {
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        scrollToTag("admin.AgentKeys", "settings never listed the account's own keys")
    }

    private fun signedInAs(name: String) {
        role(name)
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        // the read itself, not the screen: an account whose rights were
        // never read is offered nothing either, and would pass the
        // absences below
        compose.waitUntil(TIMEOUT_MS) { meReads() >= 1 }
    }

    @Test
    fun a_plain_account_is_offered_only_its_own_pages() {
        signedInAs("plain")
        openSettings()
        compose.waitForIdle()
        for (tag in listOf("admin.Accounts", "admin.Domains", "admin.Queue", "admin.Audit", "admin.Suppressed")) {
            assertFalse("$tag offered to an account that is not an administrator", offered(tag))
        }
        scrollToTag("admin.Allowed", "the account's own sender list was not offered")
    }

    @Test
    fun an_administrator_is_not_offered_other_peoples_mail_traffic() {
        signedInAs("admin")
        openSettings()
        scrollToTag("admin.Accounts", "the directory was not offered to an administrator")
        for (tag in listOf("admin.Queue", "admin.Audit", "admin.Suppressed")) {
            assertFalse("$tag offered to an administrator", offered(tag))
        }
    }

    @Test
    fun a_right_taken_away_hides_its_page_after_a_refusal() {
        signedInAs("super")
        openSettings()
        scrollToTag("admin.Queue", "the queue was not offered to a super administrator")
        role("admin")
        compose.onNodeWithTag("admin.Queue").performClick()
        // the 403 sends the app back to ask who it is
        compose.waitUntil(TIMEOUT_MS) { meReads() >= 2 }
        compose.onNodeWithTag("button.closeAdmin").performClick()
        scrollToTag("admin.Accounts", "settings never came back")
        compose.waitUntil(TIMEOUT_MS) { !offered("admin.Queue") }
    }

    @Test
    fun a_right_taken_away_hides_its_page_when_the_app_comes_back() {
        signedInAs("super")
        val before = meReads()
        role("admin")
        compose.activityRule.scenario.moveToState(Lifecycle.State.CREATED)
        compose.activityRule.scenario.moveToState(Lifecycle.State.RESUMED)
        compose.waitUntil(TIMEOUT_MS) { meReads() > before }
        openSettings()
        scrollToTag("admin.Accounts", "the directory was not offered to an administrator")
        compose.waitUntil(TIMEOUT_MS) { !offered("admin.Queue") }
    }
}
