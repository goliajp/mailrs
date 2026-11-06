package jp.golia.mailrs

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The open, empty search offers the searches saved on the web and the
 * operators the server takes; choosing a saved search runs its query.
 * The same journey as the iPhone's `SavedSearchFlowTests`.
 */
@RunWith(AndroidJUnit4::class)
class SavedSearchFlowTest : MailrsUiTest() {

    @Test
    fun a_saved_search_runs_its_query() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.onNodeWithTag("button.search").performClick()
        waitForTag("search.saved.ss_ref", "the empty search offered no saved search")
        waitForTag("search.saved.ss_ops", "a saved search with operators was left out")
        waitForTag("search.operators", "the search operators were not named")

        compose.onNodeWithTag("search.saved.ss_ref").performClick()

        waitForTag("list.searchResults", "the saved search returned nothing")
        val typed = compose.onNodeWithTag("search.field").fetchSemanticsNode()
            .config.getOrElseNullable(SemanticsProperties.EditableText) { null }?.text
        assertEquals("the field does not hold the saved query", "請求", typed)
        val inResults = hasAnyAncestor(hasTestTag("list.searchResults"))
        val narrowed = runCatching {
            compose.waitUntil(TIMEOUT_MS) {
                compose.onAllNodes(inResults and hasText("請求書のご送付につきまして")).fetchSemanticsNodes().isNotEmpty() &&
                    compose.onAllNodes(inResults and hasText("Quarterly report and the follow-up notes")).fetchSemanticsNodes().isEmpty()
            }
        }.isSuccess
        assertTrue("the results were not narrowed to what the saved query matches", narrowed)
    }
}
