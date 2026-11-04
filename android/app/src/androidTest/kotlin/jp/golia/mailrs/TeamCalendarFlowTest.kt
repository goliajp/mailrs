package jp.golia.mailrs

import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.time.DayOfWeek
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.time.LocalDate
import java.time.temporal.TemporalAdjusters

/**
 * A team conversation shows its internal notes and, when replying, who else
 * is on it; the calendar shows the week with an all-day event on its own
 * dates. The same journeys as the iPhone's `TeamCalendarFlowTests`.
 */
@RunWith(AndroidJUnit4::class)
class TeamCalendarFlowTest : MailrsUiTest() {

    private fun openTeamConversation() {
        signIn()
        waitForTag("section.reply", "no section bar over the Inbox")
        compose.onNodeWithTag("section.reply").performClick()
        waitUntilShown(hasText("Customer asks about the refund"), "the team conversation was not listed")
        compose.onNode(hasText("Customer asks about the refund")).performClick()
        waitForTag("list.messages", "the team conversation never opened")
    }

    @Test
    fun a_team_conversation_shows_its_notes() {
        openTeamConversation()
        waitUntilShown(hasTestTag("team.notes"), "no internal notes under the team conversation")
        waitUntilShown(hasTestTag("team.note.1"), "the first note was not shown")
        assertTrue("the edited note was not shown", exists(hasTestTag("team.note.2")))
        assertTrue("a note's text was not shown", exists(hasText("Refund approved, waiting on finance")))
    }

    @Test
    fun replying_says_who_else_is_writing() {
        openTeamConversation()
        tapWhenSteady("button.reply", 0)
        waitForTag("field.body", "the composer never opened")
        waitUntilShown(hasTestTag("reply.claimHint"), "replying did not say who else is on it")
        assertTrue("the hint did not name carol, who is writing", exists(hasText("carol", substring = true)))
    }

    @Test
    fun the_week_shows_an_all_day_event_on_its_own_dates() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Calendar").performScrollTo().performClick()

        val monday = LocalDate.now().with(TemporalAdjusters.previousOrSame(DayOfWeek.MONDAY))
        waitUntilShown(hasTestTag("calendar.week.$monday"), "the calendar did not open on this week")
        // the stub sends the offsite as dates, Wednesday to Friday exclusive
        waitUntilShown(hasTestTag("calendar.event.ev-offsite.${monday.plusDays(2)}"), "the all-day event is missing on Wednesday")
        assertTrue("the all-day event is missing on Thursday", exists(hasTestTag("calendar.event.ev-offsite.${monday.plusDays(3)}")))
        assertTrue("the all-day event ran into Friday", !exists(hasTestTag("calendar.event.ev-offsite.${monday.plusDays(4)}")))
        assertTrue("the all-day event started on Tuesday", !exists(hasTestTag("calendar.event.ev-offsite.${monday.plusDays(1)}")))
        assertTrue("the event with no end was not shown", exists(tagPrefix("calendar.event.ev-open.")))

        compose.onNodeWithTag("calendar.next").performClick()
        waitUntilShown(hasTestTag("calendar.week.${monday.plusDays(7)}"), "Next week did not move a week on")
    }

    private fun exists(m: SemanticsMatcher) =
        compose.onAllNodes(m, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    private fun tagPrefix(prefix: String) = SemanticsMatcher("tag starts with $prefix") { node ->
        node.config.getOrNull(androidx.compose.ui.semantics.SemanticsProperties.TestTag)?.startsWith(prefix) == true
    }

    private fun waitUntilShown(matcher: SemanticsMatcher, what: String) {
        val shown = runCatching {
            compose.waitUntil(TIMEOUT_MS) { exists(matcher) }
        }.isSuccess
        assertTrue(what, shown)
    }
}
