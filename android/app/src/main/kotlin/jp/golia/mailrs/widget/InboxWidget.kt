package jp.golia.mailrs.widget

import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.GlanceTheme
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.cornerRadius
import androidx.glance.appwidget.provideContent
import androidx.glance.appwidget.updateAll
import androidx.glance.background
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Column
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.padding
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import jp.golia.mailrs.wire.SenderIdentity

/**
 * The inbox on the home screen.
 *
 * A widget is the one Android surface a mail client can occupy without
 * being opened, and the question it answers is the one people open the
 * app to ask: *is there anything, and who from*. A count, three rows,
 * and a tap that opens the app.
 *
 * **It draws what was last fetched and never fetches.** A widget that
 * hit the network on every redraw would do so on every home-screen
 * scroll, on rotation, and whenever the launcher felt like it. The data
 * comes from [WidgetState], written by the two places that already have
 * the list in hand — a refresh in the app, and the periodic check that
 * posts the notification.
 */
class InboxWidget : GlanceAppWidget() {

    /**
     * Reading is all this does; the drawing is [InboxWidgetContent].
     *
     * Split because a composable that reaches for its own data cannot be
     * rendered by a test without arranging device state first — and what
     * a reader sees is the half worth pinning.
     */
    override suspend fun provideGlance(context: Context, id: GlanceId) {
        val snapshot = WidgetState.read(context)
        provideContent {
            // The tap is applied here rather than inside the content:
            // building the action needs a `Context`, and `LocalContext`
            // has no default in Glance's test environment — so a content
            // that reached for it could not be rendered by a test at
            // all. "No default context" was the whole failure.
            Box(GlanceModifier.fillMaxSize().clickable(OpenMailrs.action(context))) {
                InboxWidgetContent(snapshot) { threadId ->
                    OpenMailrs.action(context, threadId)
                }
            }
        }
    }
}

/**
 * The widget's contents, given what to draw.
 *
 * Pure over its snapshot: nothing here reads preferences or the
 * network, so a test can hand it three messages and check that three
 * messages come out.
 *
 * **Empty and signed-out say different things.** "Nothing unread" is
 * good news; showing it to somebody who is signed out is a lie, and
 * showing nothing at all looks broken.
 */
@Composable
fun InboxWidgetContent(
    snapshot: WidgetState.Snapshot,
    /**
     * What tapping a row does. Passed in rather than built here for
     * the same reason the whole-widget tap is: building an action
     * needs a `Context`, and `LocalContext` has no default in Glance's
     * test environment — so a content that reached for it could not be
     * rendered by a test at all.
     */
    onRow: ((String) -> androidx.glance.action.Action)? = null,
) {
    GlanceTheme {
        Column(
            GlanceModifier
                .fillMaxSize()
                .background(GlanceTheme.colors.widgetBackground)
                .cornerRadius(16.dp)
                .padding(12.dp),
        ) {
            Text(
                heading(snapshot),
                style = TextStyle(
                    color = GlanceTheme.colors.onSurface,
                    fontSize = 14.sp,
                    fontWeight = FontWeight.Medium,
                ),
            )
            for (row in snapshot.rows.take(3)) {
                val rowModifier = GlanceModifier.fillMaxWidth().padding(top = 8.dp)
                Column(onRow?.let { rowModifier.clickable(it(row.threadId)) } ?: rowModifier) {
                    Text(
                        // A name, not an address: a home screen has room
                        // for "Alice Smith" and not for the rest of it.
                        SenderIdentity.readableName(row.sender),
                        style = TextStyle(
                            color = GlanceTheme.colors.onSurface,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.Medium,
                        ),
                        maxLines = 1,
                    )
                    Text(
                        row.subject.ifBlank { "(no subject)" },
                        style = TextStyle(color = GlanceTheme.colors.onSurfaceVariant, fontSize = 12.sp),
                        maxLines = 1,
                    )
                }
            }
            if (snapshot.signedIn && snapshot.rows.isEmpty()) {
                Column(
                    GlanceModifier.fillMaxSize(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        "Nothing unread.",
                        style = TextStyle(color = GlanceTheme.colors.onSurfaceVariant, fontSize = 12.sp),
                    )
                }
            }
        }
    }
}

private fun heading(snapshot: WidgetState.Snapshot): String = when {
    !snapshot.signedIn -> "Sign in to Mailrs"
    snapshot.unread == 0 -> "Mailrs"
    snapshot.unread == 1 -> "1 unread"
    else -> "${snapshot.unread} unread"
}

/**
 * What the launcher talks to.
 *
 * Separate from the widget because the system instantiates this by name
 * from the manifest, and a receiver that also held the drawing would be
 * re-created for every update.
 */
class InboxWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = InboxWidget()
}

/** Redraw every placed widget. Safe to call when none are placed. */
suspend fun refreshInboxWidgets(context: Context) {
    runCatching { InboxWidget().updateAll(context) }
}
