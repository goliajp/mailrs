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
 * The operator's screens.
 *
 * Split out of `MailFlowTest` when it reached 1,294 lines against this
 * repo's 500-line limit, and split here because these are a different
 * job: nothing in this file is on the path a person takes to read their
 * mail.
 */
@RunWith(AndroidJUnit4::class)
class AdminFlowTest : MailrsUiTest() {

    /**
     * The operator lists, and what deleting one names.
     *
     * A row's key is the identity the server knows the thing by — an
     * alias id, a domain name — kept beside the row rather than derived
     * from the text on it, so a row whose display changes still deletes
     * the right thing. The assertion is that the list came back one
     * shorter after a re-read, which is the server's answer rather than
     * the screen's.
     */
    @Test
    fun the_operator_lists_load_and_a_delete_names_its_row() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Accounts", "settings never listed the operator sections")

        compose.onNodeWithTag("admin.Accounts").performClick()
        waitForTag("list.admin", "the accounts never listed")
        compose.onNodeWithText("lihao@golia.jp").assertIsDisplayed()

        pressBack()
        waitForTag("admin.Domains", "back did not return to settings")
        compose.onNodeWithTag("admin.Domains").performClick()
        waitForTag("list.admin", "the domains never listed")
        val before = compose.onAllNodesWithTag("row.admin").fetchSemanticsNodes().size
        assertTrue("the fixture has no domains", before > 0)

        compose.onAllNodesWithTag("button.deleteAdminRow").onFirst().performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.admin").fetchSemanticsNodes().size == before - 1
        }
    }

    /**
     * The queue tells stuck apart from asked-for-later.
     *
     * The fixture holds one of each and a third in flight. Before the
     * row read its own timestamps the scheduled one was
     * indistinguishable from the stuck one, and a queue where every row
     * looks stuck is a queue nobody reads — so the assertion is on the
     * words beside the rows, not on how many there are.
     */
    @Test
    fun the_queue_says_which_rows_are_stuck() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Queue", "settings never listed the queue")

        compose.onNodeWithTag("admin.Queue").performClick()
        waitForTag("list.admin", "the queue never listed")

        compose.onNodeWithText("stuck@example.com").assertIsDisplayed()
        compose.onNodeWithText("attempt 3 — 421 too many connections").assertIsDisplayed()
        compose.onAllNodesWithText("scheduled for", substring = true).onFirst().assertIsDisplayed()
    }

    @Test
    fun a_dmarc_row_reads_as_passing_against_total() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Dmarc", "settings never listed DMARC")

        compose.onNodeWithTag("admin.Dmarc").performClick()
        waitForTag("list.admin", "the reports never listed")
        compose.onNodeWithText("google.com").assertIsDisplayed()
        compose.onNodeWithText("118/120 passing · p=quarantine").assertIsDisplayed()
    }

    /**
     * Adding an alias, with the domain taken from the address.
     *
     * The two cannot disagree, so the form does not ask twice — a form
     * that lets them is a form that will be filled in wrong. The
     * assertion is that the list came back one longer, which is the
     * server's answer rather than the dialog's.
     */
    @Test
    fun an_alias_can_be_added_from_the_phone() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Aliases", "settings never listed the aliases")
        compose.onNodeWithTag("admin.Aliases").performClick()
        waitForTag("list.admin", "the aliases never listed")

        val before = compose.onAllNodesWithTag("row.admin").fetchSemanticsNodes().size
        compose.onNodeWithTag("button.addAdminRow").performClick()
        waitForTag("field.admin0", "the form never opened")

        // Half filled: the button must not be live yet.
        compose.onNodeWithTag("field.admin0").performTextInput("help@golia.jp")
        compose.onNodeWithTag("button.confirmAdmin").assertIsNotEnabled()
        compose.onNodeWithTag("field.admin1").performTextInput("lihao@golia.jp")
        compose.onNodeWithTag("button.confirmAdmin").performClick()

        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.admin").fetchSemanticsNodes().size == before + 1
        }
        compose.onNodeWithText("help@golia.jp → lihao@golia.jp").assertIsDisplayed()
    }

    /**
     * The allow list reads `entries`, not `items`.
     *
     * `spam_lists.rs` answers with a different key from the admin
     * lists, and reaching for the wrong one decodes an empty list —
     * which on screen is indistinguishable from "nothing is listed".
     * So this asserts the address is there, and then that adding one
     * and removing it both reach the server.
     */
    @Test
    fun the_allow_list_loads_and_can_be_edited() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Allowed", "settings never listed the allow list")
        compose.onNodeWithTag("admin.Allowed").performClick()
        waitForTag("list.admin", "the allow list never loaded")
        compose.onNodeWithText("friend@example.com").assertIsDisplayed()

        compose.onNodeWithTag("button.addAdminRow").performClick()
        waitForTag("field.admin0", "the form never opened")
        compose.onNodeWithTag("field.admin0").performTextInput("newfriend@example.com")
        compose.onNodeWithTag("button.confirmAdmin").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("newfriend@example.com").fetchSemanticsNodes().isNotEmpty()
        }

        compose.onAllNodesWithTag("button.deleteAdminRow").onFirst().performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("friend@example.com").fetchSemanticsNodes().isEmpty()
        }
    }

    @Test
    fun agent_keys_name_what_they_can_do() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.AgentKeys", "settings never listed the keys")
        compose.onNodeWithTag("admin.AgentKeys").performClick()
        waitForTag("list.admin", "the keys never listed")

        compose.onNodeWithText("Scheduler").assertIsDisplayed()
        compose.onNodeWithText("mk_a1b2c · mail.send").assertIsDisplayed()
    }

    @Test
    fun permission_groups_say_which_are_built_in() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        compose.onNodeWithTag("admin.Groups").performScrollTo().performClick()
        waitForTag("list.admin", "the groups never listed")

        compose.onNodeWithText("Administrators").assertIsDisplayed()
        compose.onNodeWithText("built in").assertIsDisplayed()
    }

    /**
     * A group is a list with a list inside it, and the inner one is the
     * point: "Support" says nothing, its members are what somebody came
     * for. Adding one goes to the server and the list is re-read.
     */
    @Test
    fun an_email_group_opens_to_its_members() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        compose.onNodeWithTag("admin.EmailGroups").performScrollTo().performClick()
        waitForTag("list.admin", "the email groups never listed")

        compose.onAllNodesWithTag("row.admin").onFirst().performClick()
        waitForTag("list.groupMembers", "the group never opened")
        compose.onNodeWithText("lihao@golia.jp").assertIsDisplayed()

        val before = compose.onAllNodesWithTag("row.member").fetchSemanticsNodes().size
        compose.onNodeWithTag("button.addMember").performClick()
        waitForTag("field.member", "the form never opened")
        compose.onNodeWithTag("field.member").performTextInput("newcomer@golia.jp")
        compose.onNodeWithTag("button.confirmMember").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.member").fetchSemanticsNodes().size == before + 1
        }
    }

    /**
     * A permission group shows what it grants and offers no edit.
     *
     * Its membership decides what somebody may *do*, and granting that
     * from a phone list — no confirmation, no record of why — is not an
     * edit this offers. So the grants are on screen and the add button
     * is not.
     */
    @Test
    fun a_permission_group_shows_its_grants_and_cannot_be_edited() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        compose.onNodeWithTag("admin.Groups").performScrollTo().performClick()
        waitForTag("list.admin", "the groups never listed")

        compose.onAllNodesWithTag("row.admin").onFirst().performClick()
        waitForTag("list.groupMembers", "the group never opened")
        compose.onNodeWithText("admin.accounts").assertIsDisplayed()
        compose.onAllNodesWithTag("button.addMember").assertCountEquals(0)
    }

    /**
     * An account opens to the three things kept away from its row: what
     * it may hold, the rule that files its mail, and what subscribes to
     * it.
     *
     * The sieve script is the reason this screen exists — an operator
     * asking "why did that go to Ops" wants to read the rule. All three
     * are read-only, and the webhook's signing secret is on the wire and
     * deliberately not on the screen.
     */
    @Test
    fun an_account_opens_to_its_quota_sieve_and_webhooks() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Accounts", "settings never listed the accounts")
        compose.onNodeWithTag("admin.Accounts").performClick()
        waitForTag("list.admin", "the accounts never listed")

        compose.onAllNodesWithTag("row.admin").onFirst().performClick()
        waitForTag("account.detail", "the account never opened")

        compose.onNodeWithTag("account.quota").assertTextContains("5.4 GB")
        compose.onNodeWithTag("account.sieve").assertTextContains("fileinto", substring = true)
        compose.onNodeWithText("https://hooks.example/mail").assertIsDisplayed()
        // The secret proves a delivery came from this server. A screen
        // that prints it turns a glance over a shoulder into a forgery.
        compose.onAllNodesWithText("whsec_x", substring = true).assertCountEquals(0)
    }

    @Test
    fun apps_name_their_owner_and_scopes() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        compose.onNodeWithTag("admin.Apps").performScrollTo().performClick()
        waitForTag("list.admin", "the apps never listed")

        compose.onNodeWithText("Reporting").assertIsDisplayed()
        compose.onNodeWithText("lihao@golia.jp · mail.read").assertIsDisplayed()
    }

    /**
     * Settings, and the way out of the account.
     *
     * Sign out used to be a text button beside refresh — one mis-tap
     * from losing the session on the screen used most. It lives at the
     * bottom of settings now, so the path a person actually takes is
     * the one this walks.
     */
    @Test
    fun settings_holds_the_account_and_the_way_out() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("appearance.System", "settings never opened")

        // Choosing dark must not be a no-op the next screen forgets.
        compose.onNodeWithTag("appearance.Dark").performClick()
        // Scrolled to first: settings is longer than a phone, and a tap
        // at a coordinate below the edge is not the tap this means.
        compose.onNodeWithTag("button.signOut").performScrollTo().performClick()
        waitForTag("field.address", "signing out did not return to sign-in")
    }

    /**
     * The appearance choice is written where the next launch reads it.
     *
     * The screen showing dark is not the same as the phone remembering
     * it: `chooseAppearance` writes through to `SharedPreferences` as
     * it is chosen, and the next launch builds its first state from
     * there. This taps Dark and then asks the store — a fresh `Prefs`
     * over the same file, which is what a cold start does.
     */
    @Test
    fun choosing_dark_is_written_where_the_next_launch_reads_it() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("appearance.Dark", "settings never opened")

        val context = InstrumentationRegistry.getInstrumentation().targetContext
        compose.onNodeWithTag("appearance.Dark").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            jp.golia.mailrs.wire.Prefs(context).appearance == jp.golia.mailrs.wire.Prefs.Appearance.Dark
        }

        // And back, so the next test does not inherit a dark phone.
        compose.onNodeWithTag("appearance.System").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            jp.golia.mailrs.wire.Prefs(context).appearance == jp.golia.mailrs.wire.Prefs.Appearance.System
        }
    }

    /**
     * The splash screen is wired, which is all there is to check.
     *
     * What the system draws before the first frame is not visible to
     * this suite, but the two things that make it the app's own are:
     * the activity launches with the splash theme, and that theme hands
     * over to the real one. Both are declarations, and a declaration
     * that is missing is exactly how an app ends up with the white
     * rectangle again.
     */
    @Test
    fun the_activity_launches_with_the_splash_theme() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val activity = context.packageManager.getActivityInfo(
            android.content.ComponentName(context, MainActivity::class.java),
            0,
        )
        val expected = context.resources.getIdentifier(
            "Theme.Mailrs.Splash",
            "style",
            context.packageName,
        )
        assertTrue("Theme.Mailrs.Splash is not declared", expected != 0)
        assertEquals("the activity does not launch with the splash theme", expected, activity.theme)

        // And it hands over: without `postSplashScreenTheme` the splash
        // theme stays up and the app wears it.
        val styled = context.obtainStyledAttributes(
            expected,
            intArrayOf(context.resources.getIdentifier("postSplashScreenTheme", "attr", context.packageName)),
        )
        val handover = styled.getResourceId(0, 0)
        styled.recycle()
        assertTrue("the splash theme does not hand over to another", handover != 0)
    }
}
