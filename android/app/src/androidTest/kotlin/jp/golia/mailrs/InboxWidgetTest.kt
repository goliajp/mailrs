package jp.golia.mailrs

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
}
