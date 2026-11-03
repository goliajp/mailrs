package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** The shapes filing mail reads and writes, beside [Wire]'s. */
object WireTriage {
    /** One of `GET /api/labels` — `crates/mailbox/src/labels.rs`, `Label`. */
    @Serializable
    data class Label(
        val id: String,
        val name: String,
        val color: String,
        @SerialName("created_at") val createdAt: Long = 0,
    )

    /** `GET /api/quarantine` — the held conversations, newest first. */
    @Serializable
    data class HeldList(
        val items: List<Wire.Conversation>,
        val total: Int = 0,
    )

    /** `PUT /api/conversations/{id}/snooze`. */
    @Serializable
    data class SnoozeBody(
        @SerialName("snoozed_until") val snoozedUntil: Long,
    )
}
