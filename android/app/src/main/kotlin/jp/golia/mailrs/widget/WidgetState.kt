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
    data class Row(val sender: String, val subject: String)

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

    /** Called with whatever the caller has just fetched anyway. */
    fun write(context: Context, signedIn: Boolean, conversations: List<Wire.Conversation>) {
        val unread = conversations.count { it.unreadCount > 0 }
        val snapshot = Snapshot(
            signedIn = signedIn,
            unread = unread,
            rows = conversations
                .filter { it.unreadCount > 0 }
                .sortedByDescending { it.lastDate }
                .take(3)
                .map { Row(it.participants.firstOrNull().orEmpty(), it.subject) },
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
