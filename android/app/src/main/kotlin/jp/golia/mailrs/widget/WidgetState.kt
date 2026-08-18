package jp.golia.mailrs.widget

import android.content.Context
import jp.golia.mailrs.wire.Wire
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * What the home-screen widget draws, written by whoever last knew.
 *
 * A widget cannot fetch — it is redrawn on every home-screen scroll,
 * on rotation, and whenever the launcher decides — so this is a small
 * snapshot that the app and the periodic check write, and the widget
 * only reads.
 *
 * **Signed out is a state, not an absence.** A widget that showed
 * "Nothing unread" to somebody who is signed out would be lying, and
 * one that showed nothing at all would look broken.
 */
object WidgetState {

    @Serializable
    /**
     * @param threadId what tapping this row opens. A home screen that
     *   lists three subjects and opens the same inbox for all of them
     *   is naming things it cannot reach.
     */
    data class Row(val threadId: String, val sender: String, val subject: String)

    @Serializable
    data class Snapshot(
        val signedIn: Boolean = false,
        val unread: Int = 0,
        val rows: List<Row> = emptyList(),
    )

    private val json = Json { ignoreUnknownKeys = true }

    fun read(context: Context): Snapshot {
        val raw = prefs(context).getString(KEY, null) ?: return Snapshot()
        return runCatching { json.decodeFromString(Snapshot.serializer(), raw) }.getOrDefault(Snapshot())
    }

    /**
     * Enough for the tallest widget a launcher will hand out. Kept
     * small: this is a preference blob read on every redraw, not a
     * mailbox.
     */
    const val MAX_ROWS = 9

    /** Called with whatever the caller has just fetched anyway. */
    fun write(context: Context, signedIn: Boolean, conversations: List<Wire.Conversation>) {
        val unread = conversations.count { it.unreadCount > 0 }
        val snapshot = Snapshot(
            signedIn = signedIn,
            unread = unread,
            rows = conversations
                .filter { it.unreadCount > 0 }
                .sortedByDescending { it.lastDate }
                // As many as the tallest widget can show, not as many
                // as the smallest: what is stored has to cover every
                // size the launcher may give, and three was the number
                // the *drawing* used before it learned to measure.
                .take(MAX_ROWS)
                .map { Row(it.threadId, it.participants.firstOrNull().orEmpty(), it.subject) },
        )
        prefs(context).edit()
            .putString(KEY, json.encodeToString(Snapshot.serializer(), snapshot))
            .apply()
    }

    /** Signing out empties it — the next person's launcher is not shown this one's mail. */
    fun clear(context: Context) {
        prefs(context).edit().remove(KEY).apply()
    }

    private fun prefs(context: Context) =
        context.getSharedPreferences("mailrs.widget", Context.MODE_PRIVATE)

    private const val KEY = "snapshot"
}
