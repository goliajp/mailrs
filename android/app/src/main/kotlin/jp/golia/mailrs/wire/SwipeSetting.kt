package jp.golia.mailrs.wire

import androidx.annotation.StringRes
import jp.golia.mailrs.R
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * What a swipe on a conversation row does. The reader's setting, kept on
 * the server (`/api/settings/swipe`) so the web and both apps agree; the
 * choices and the default are the `swipe` block of `actions.json`. The
 * same type as the iPhone's `Wire/SwipeSetting.swift`.
 */
@Serializable
enum class SwipeChoice(@StringRes val label: Int) {
    @SerialName("archive") Archive(R.string.action_archive),
    @SerialName("trash") Trash(R.string.action_trash),
    @SerialName("read") Read(R.string.swipe_read),
    @SerialName("none") None(R.string.swipe_none),
    ;

    /**
     * The action it runs on [c], or null for none.
     *
     * In the Trash archive and trash both restore: a swipe never deletes,
     * because deleting for good is the one thing nothing takes back.
     */
    fun action(c: Wire.Conversation, list: MailList): RowAction? = when (this) {
        None -> null
        Read -> when (c.unreadCount > 0) {
            true -> RowAction.Read
            false -> RowAction.Unread
        }
        Archive, Trash -> filing(c, list)
    }

    private fun filing(c: Wire.Conversation, list: MailList): RowAction = when {
        c.trashedAt > 0 || list == MailList.Trash -> RowAction.Untrash
        this == Trash -> RowAction.Trash
        c.archived -> RowAction.Unarchive
        else -> RowAction.Archive
    }
}

/** Backend: `core-sidestate/families/swipe.rs` — `Swipe`. Right is a finger moving left to right. */
@Serializable
data class SwipeSetting(val left: SwipeChoice, val right: SwipeChoice) {
    companion object {
        val Standard = SwipeSetting(left = SwipeChoice.Trash, right = SwipeChoice.Archive)
    }
}

suspend fun MailrsClient.swipeSetting() = one(get("/api/settings/swipe"), SwipeSetting.serializer())

suspend fun MailrsClient.saveSwipeSetting(s: SwipeSetting) =
    one(call("PUT", "/api/settings/swipe", settingsWrites.encodeToString(SwipeSetting.serializer(), s)), SwipeSetting.serializer())
