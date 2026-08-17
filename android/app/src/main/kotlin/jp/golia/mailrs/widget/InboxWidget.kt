package jp.golia.mailrs.widget

import android.content.Context
import androidx.compose.ui.graphics.Color
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
 * app to ask: *is there anything, and who from*. Three rows and a
 * count, and a tap that opens the app.
 *
 * **It draws what was last fetched and never fetches.** A widget that
 * hits the network on every redraw would do so on every home-screen
 * scroll, on rotation, and whenever the launcher feels like it. The
 * data comes from [WidgetState], written by the same periodic check
 * that posts the notification and by the app itself; here it is only
 * read.
 *
 * **Empty and signed-out say different things.** "No unread mail" is
 * good news; a widget showing it to somebody who is signed out is a
 * lie, and one showing nothing at all looks broken.
 */
class InboxWidget : GlanceAppWidget() {

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        val snapshot = WidgetState.read(context)
        provideContent {
            GlanceTheme {
                Column(
                    GlanceModifier
                        .fillMaxSize()
                        .background(GlanceTheme.colors.widgetBackground)
                        .cornerRadius(16.dp)
                        .padding(12.dp)
                        .clickable(OpenMailrs.action(context)),
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
                        Column(GlanceModifier.fillMaxWidth().padding(top = 8.dp)) {
                            Text(
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
                                style = TextStyle(
                                    color = GlanceTheme.colors.onSurfaceVariant,
                                    fontSize = 12.sp,
                                ),
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
    }

    private fun heading(snapshot: WidgetState.Snapshot): String = when {
        !snapshot.signedIn -> "Sign in to Mailrs"
        snapshot.unread == 0 -> "Mailrs"
        snapshot.unread == 1 -> "1 unread"
        else -> "${snapshot.unread} unread"
    }

    private companion object {
        /** Unused today; kept so a future accent has one place to live. */
        val ACCENT = Color(0xFF3B7DDD)
    }
}

/**
 * What the launcher talks to.
 *
 * Separate from the widget itself because the system instantiates this
 * by name from the manifest, and a receiver that also held the drawing
 * would be re-created for every update.
 */
class InboxWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = InboxWidget()
}

/** Redraw every placed widget. Safe to call when none are placed. */
suspend fun refreshInboxWidgets(context: Context) {
    runCatching { InboxWidget().updateAll(context) }
}
