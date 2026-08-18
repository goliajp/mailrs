package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What a list looks like, rather than what it says.
 *
 * Two properties that no assertion about content can reach: a row
 * keeps its shape whatever arrives in it, and a list has one fewer
 * rule than it has rows. Both were found by looking at a screenshot,
 * and both are counted here so they do not need looking at again.
 */
@RunWith(AndroidJUnit4::class)
class ListShapeTest : MailrsUiTest() {

    /**
     * A row stays a row, whatever arrives in it.
     *
     * The fixtures are otherwise comfortable — short subjects, one
     * sender, ordinary words — and layout breaks at the edges. The
     * first conversation is deliberately extreme: a subject three
     * times longer than any screen, forty participants, and a snippet
     * with no spaces to break at. All three are the kind of thing a
     * mailing list produces on an ordinary Tuesday.
     */
    @Test
    fun an_extreme_conversation_still_fits_its_row() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        val rows = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes()
        assertTrue("nothing listed", rows.size >= 3)
        val heights = rows.map { it.boundsInRoot.height }
        // The extreme row is the **last** — deliberately the oldest, so
        // it does not become the newest unread that the notification
        // names and that half the flow tests open first. Within a
        // fraction of its neighbours: a subject that wrapped instead of
        // truncating, or forty senders spelled out, would make it
        // taller than the rest and push everything below off screen.
        assertTrue(
            "the extreme row is $heights, which is not the shape of the others",
            heights.last() <= heights.first() * 1.2f,
        )
    }

    /**
     * A list has one fewer rule than it has rows.
     *
     * Every list drew a divider after each row, the last one included,
     * so a rule sat under the final row with nothing beneath it —
     * which reads as a list that was cut off rather than one that
     * ended. Counted rather than looked at: the number of dividers is
     * the assertion, and it is exactly rows minus one.
     */
    @Test
    fun the_last_row_of_a_list_has_no_rule_under_it() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        val rows = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size
        val rules = compose.onAllNodesWithTag("divider.row", useUnmergedTree = true)
            .fetchSemanticsNodes().size
        assertTrue("nothing listed", rows > 1)
        assertEquals("a list of $rows rows drew $rules rules", rows - 1, rules)
    }
}
