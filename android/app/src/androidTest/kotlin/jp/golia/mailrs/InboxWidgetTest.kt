package jp.golia.mailrs

import org.junit.Assert.assertTrue
import androidx.compose.ui.unit.dp
import jp.golia.mailrs.widget.OpenMailrs
import jp.golia.mailrs.wire.NewMailWorker
import org.junit.Assert.assertNull
import org.junit.Assert.assertEquals
import androidx.glance.testing.unit.hasText
import androidx.glance.appwidget.testing.unit.runGlanceAppWidgetUnitTest
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import jp.golia.mailrs.widget.InboxWidgetContent
import jp.golia.mailrs.widget.WidgetState
import jp.golia.mailrs.wire.Wire
import jp.golia.mailrs.widget.InboxWidgetReceiver
import org.junit.Assert.assertNotEquals
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What the widget actually draws.
 *
 * `WidgetStateTest` pins the snapshot it is handed; this pins what
 * comes out the other side, which is the half a reader sees. Without it
 * the widget could be given the right three messages and render an
 * empty box, and nothing would say so.
 */
@RunWith(AndroidJUnit4::class)
class InboxWidgetTest {

    private val context = ApplicationProvider.getApplicationContext<android.content.Context>()

    private fun conversation(
        subject: String,
        unread: Int,
        date: Long,
        sender: String = "Alice Smith <alice@example.com>",
    ) = Wire.Conversation(
        threadId = subject,
        subject = subject,
        participants = listOf(sender),
        messageCount = 1,
        unreadCount = unread,
        lastDate = date,
        category = "inbox",
        flagged = false,
        snippet = "",
        pinned = false,
        archived = false,
        importanceLevel = "normal",
        importanceScore = 0f,
        requiresAction = false,
        receivedCount = 1,
        sentCount = 0,
    )

    /**
     * The sender is a name, not an address: `SenderIdentity` is what
     * turns "Alice Smith <alice@example.com>" into something a home
     * screen has room for.
     */
    @Test
    fun it_draws_the_count_and_who_the_mail_is_from() = runGlanceAppWidgetUnitTest {
        WidgetState.write(
            context,
            signedIn = true,
            conversations = listOf(
                conversation("Quarterly report", unread = 1, date = 200),
                // A different sender, because `onNode` wants exactly one
                // match: with both rows from Alice it found two and
                // failed as "Failed assertExists", which reads as *not
                // drawn* and meant *drawn twice*.
                conversation("Invoice", unread = 1, date = 100, sender = "Keiri <keiri@example.co.jp>"),
            ),
        )

        provideComposable { InboxWidgetContent(WidgetState.read(context)) }

        // Asserted one at a time with the reason attached: `assertExists`
        // says only "Failed assertExists", so a three-line block names
        // nothing when it goes red.
        for (expected in listOf("2 unread", "Alice Smith", "Quarterly report", "Keiri")) {
            runCatching { onNode(hasText(expected)).assertExists() }
                .onFailure { throw AssertionError("the widget did not draw \"$expected\"", it) }
        }
    }

    /** Signed out says so rather than claiming an empty inbox. */
    @Test
    fun signed_out_says_so() = runGlanceAppWidgetUnitTest {
        WidgetState.clear(context)

        provideComposable { InboxWidgetContent(WidgetState.read(context)) }

        onNode(hasText("Sign in to Mailrs")).assertExists()
    }

    /**
     * A taller widget shows more mail.
     *
     * It declares itself resizable in both directions and drew three
     * rows whatever it was dragged to — a resize handle that changes
     * nothing. The arithmetic is `WidgetRows`; this is the wiring,
     * which is the half that can be absent while the rule is right.
     *
     * Two tests rather than one, because a size can only be set before
     * the content is provided and each environment allows it once.
     */
    @Test
    fun a_short_widget_draws_one_row() = runGlanceAppWidgetUnitTest {
        setAppWidgetSize(androidx.compose.ui.unit.DpSize(200.dp, 110.dp))
        provideComposable { InboxWidgetContent(sixMessages()) }
        assertEquals(1, (1..6).count { runCatching { onNode(hasText("Subject $it")).assertExists() }.isSuccess })
    }

    @Test
    fun a_tall_widget_draws_several() = runGlanceAppWidgetUnitTest {
        setAppWidgetSize(androidx.compose.ui.unit.DpSize(200.dp, 300.dp))
        provideComposable { InboxWidgetContent(sixMessages()) }
        val drawn = (1..6).count { runCatching { onNode(hasText("Subject $it")).assertExists() }.isSuccess }
        assertTrue("a tall widget drew $drawn rows", drawn >= 5)
    }

    /**
     * The widget picker shows what the widget looks like.
     *
     * Without `previewLayout` the picker falls back to
     * `initialLayout`, which for a Glance widget is its loading state —
     * an empty box, shown at the one moment somebody is deciding
     * whether to put this on their home screen.
     */
    @Test
    fun the_picker_has_something_to_show() {
        val info = android.appwidget.AppWidgetManager.getInstance(context)
            .getInstalledProvidersForPackage(context.packageName, null)
            .single { it.provider.className == InboxWidgetReceiver::class.java.name }
        assertNotEquals("the widget offers the picker no preview", 0, info.previewLayout)
    }

    /**
     * A tapped row asks for that conversation, by the same route a
     * notification does.
     *
     * Two ways into one activity is how one of them ends up handled
     * and the other forgotten — this asserts the widget uses the
     * notification's extra rather than an extra of its own.
     */
    @Test
    fun a_row_opens_its_own_conversation() {
        assertEquals(
            "a tapped row must ask for its own conversation",
            "t7",
            OpenMailrs.intent(context, "t7").getStringExtra(NewMailWorker.EXTRA_THREAD_ID),
        )
        // And the whole-widget tap still means "just open the app".
        assertNull(OpenMailrs.intent(context).getStringExtra(NewMailWorker.EXTRA_THREAD_ID))
    }

    private fun sixMessages(): WidgetState.Snapshot {
        WidgetState.write(
            context,
            signedIn = true,
            conversations = (1..6).map {
                conversation("Subject $it", unread = 1, date = 100L * it, sender = "S$it <s$it@example.com>")
            },
        )
        return WidgetState.read(context)
    }
}
