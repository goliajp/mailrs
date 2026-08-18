package jp.golia.mailrs

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.printToString
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Every control has to say what it is.
 *
 * A button whose whole content is an icon is, to a screen reader, a
 * button called nothing — and this app is mostly icons: archive, star,
 * reply, the drawer, the overflow. Checking them by hand means checking
 * them once, so this walks the tree instead and fails on the first one
 * that cannot introduce itself.
 *
 * It is a sweep rather than a list of expected labels: a new icon
 * button added next week is caught without anybody remembering to add
 * it here, which is the only version of this test worth having.
 */
@RunWith(AndroidJUnit4::class)
class AccessibilityTest : GrantsNotifications() {

    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    @Before
    fun startSignedOut() {
        compose.activityRule.scenario.onActivity { it.signOutForTest() }
    }

    @Test
    fun every_control_on_the_sign_in_screen_is_named() {
        compose.waitUntil(TIMEOUT) {
            compose.onAllNodes(hasTestTag("field.address")).fetchSemanticsNodes().isNotEmpty()
        }
        assertEveryClickableIsNamed("sign in")
    }

    @Test
    fun every_control_on_the_inbox_is_named() {
        signIn()
        try {
            compose.waitUntil(TIMEOUT) {
                compose.onAllNodes(hasTestTag("list.conversations")).fetchSemanticsNodes().isNotEmpty()
            }
        } catch (e: Throwable) {
            val error = compose.onAllNodes(hasTestTag("text.signInError")).fetchSemanticsNodes()
                .firstOrNull()?.config?.getOrNull(SemanticsProperties.Text)?.joinToString()
            throw AssertionError("the inbox never listed. sign-in said: $error", e)
        }
        assertEveryClickableIsNamed("the inbox")
    }

    @Test
    fun every_control_in_a_thread_is_named() {
        signIn()
        compose.waitUntil(TIMEOUT) {
            compose.onAllNodes(hasTestTag("row.conversation")).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        compose.waitUntil(TIMEOUT) {
            compose.onAllNodes(hasTestTag("list.messages")).fetchSemanticsNodes().isNotEmpty()
        }
        assertEveryClickableIsNamed("a thread")
    }

    @Test
    fun every_control_in_the_composer_is_named() {
        signIn()
        compose.waitUntil(TIMEOUT) {
            compose.onAllNodes(hasTestTag("button.compose")).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithTagOrFail("button.compose").performClick()
        compose.waitUntil(TIMEOUT) {
            compose.onAllNodes(hasTestTag("field.to")).fetchSemanticsNodes().isNotEmpty()
        }
        assertEveryClickableIsNamed("the composer")
    }

    /**
     * A clickable node passes if it, or anything under it, carries text
     * or a content description — which is what a screen reader reads
     * out. Editable fields are exempt: their label is their own text,
     * and an empty one that has not been typed into yet is not a
     * defect.
     */
    /**
     * Triage is reachable without a gesture.
     *
     * Archive and mark-read are on a swipe, and a swipe is the one
     * thing a screen reader takes over: TalkBack consumes the gesture
     * for its own navigation, so a row whose only path to filing is a
     * swipe has no path at all for the person using it. Android's
     * answer is a custom accessibility action, which TalkBack offers
     * from its actions menu.
     */
    @Test
    fun a_row_can_be_filed_without_swiping() {
        signIn()
        waitUntilDisplayed("list.conversations")
        val row = compose.onAllNodesWithTag("row.conversation")
            .fetchSemanticsNodes()
            .first()
        val actions = row.config.getOrNull(SemanticsActions.CustomActions).orEmpty().map { it.label }
        assertTrue("a row offers no way to file it but a swipe: $actions", "Archive" in actions)
        assertTrue("a row offers no way to mark it read but a swipe: $actions", "Mark read" in actions)

        // Present is not the same as wired. An action whose lambda goes
        // nowhere reads exactly like one that works, from here and from
        // TalkBack alike.
        val before = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size
        compose.runOnUiThread {
            row.config[SemanticsActions.CustomActions].first { it.label == "Archive" }.action()
        }
        compose.waitUntil(10_000) {
            compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size < before
        }
    }

    private fun assertEveryClickableIsNamed(where: String) {
        val unnamed = mutableListOf<String>()
        walk(compose.onRoot().fetchSemanticsNode()) { node ->
            val clickable = node.config.getOrNull(SemanticsActions.OnClick) != null
            if (!clickable) return@walk
            if (node.config.getOrNull(SemanticsProperties.EditableText) != null) return@walk
            if (named(node)) return@walk
            unnamed += node.config.getOrNull(SemanticsProperties.TestTag) ?: "#${node.id}"
        }
        assertTrue(
            "controls on $where that a screen reader cannot name: $unnamed",
            unnamed.isEmpty(),
        )
    }

    private fun named(node: SemanticsNode): Boolean {
        val hasText = !node.config.getOrNull(SemanticsProperties.Text).isNullOrEmpty()
        val hasDescription =
            !node.config.getOrNull(SemanticsProperties.ContentDescription).isNullOrEmpty()
        if (hasText || hasDescription) return true
        return node.children.any { named(it) }
    }

    private fun walk(node: SemanticsNode, visit: (SemanticsNode) -> Unit) {
        visit(node)
        node.children.forEach { walk(it, visit) }
    }

    /**
     * Wait for the field to be **on screen**, not merely present, and
     * then for an answer of either kind.
     *
     * The short version — type, click, wait for the inbox — left this
     * class failing one test in four with the app still sitting on the
     * sign-in screen: signing out slides that screen back in, and a tap
     * dispatched across the animation lands where the button was going
     * to be. The same lesson `MailFlowTest` learned; kept here rather
     * than shared, because the two classes reach the rule differently.
     */
    private fun signIn() {
        val stub = StubServer.base()
        compose.activityRule.scenario.onActivity { it.useStubServer(stub) }
        waitUntilDisplayed("field.address")
        compose.onNodeWithTagOrFail("field.address").performTextInput("me@golia.jp")
        compose.onNodeWithTagOrFail("field.password").performTextInput("anything")
        waitUntilDisplayed("button.signIn")
        compose.onNodeWithTagOrFail("button.signIn").performClick()
        compose.waitUntil(TIMEOUT) {
            compose.onAllNodes(hasTestTag("row.conversation")).fetchSemanticsNodes().isNotEmpty() ||
                compose.onAllNodes(hasTestTag("text.signInError")).fetchSemanticsNodes().isNotEmpty()
        }
    }

    private fun waitUntilDisplayed(tag: String) {
        compose.waitUntil(TIMEOUT) {
            compose.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty() &&
                runCatching { compose.onNodeWithTagOrFail(tag).assertIsDisplayed() }.isSuccess
        }
    }

    private fun androidx.compose.ui.test.junit4.AndroidComposeTestRule<*, *>.onNodeWithTagOrFail(
        tag: String,
    ) = onAllNodes(hasTestTag(tag)).onFirst()

    private companion object {
        const val TIMEOUT = 15_000L
    }
}
