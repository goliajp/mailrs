package jp.golia.mailrs.wire

import androidx.annotation.StringRes
import jp.golia.mailrs.R

/** The word a Sent-list row shows for a send's delivery state. */
object SendStatus {
    /**
     * The string for `status` as `/api/mail/sends` gives it, or null
     * for a value this build does not know; the row then shows the wire
     * word as it came, rather than hiding a state it cannot name.
     */
    @StringRes
    fun label(status: String): Int? = when (status) {
        "failed" -> R.string.sent_status_failed
        "partial" -> R.string.sent_status_partial
        "sending" -> R.string.sent_status_sending
        "scheduled" -> R.string.sent_status_scheduled
        "delivered" -> R.string.sent_status_delivered
        "cancelled" -> R.string.sent_status_cancelled
        else -> null
    }

    enum class Tone { Success, Warning, Danger, Muted }

    /** Partly refused is not a failure, and not an arrival either. */
    fun tone(status: String): Tone = when (status) {
        "delivered", "sent" -> Tone.Success
        "partial" -> Tone.Warning
        "failed", "bounced" -> Tone.Danger
        else -> Tone.Muted
    }

    fun text(status: String): String {
        val id = label(status) ?: return status
        return Texts.get(id)
    }
}
