package jp.golia.mailrs.wire

/**
 * What to offer a reader who wants off a list, given what the message
 * said.
 *
 * Ported from `ios/Mailrs/Wire/UnsubscribeOffer.swift`. Three answers,
 * not one, because the three cost the reader different things and only
 * one of them is free:
 *
 * - **One-click** — the server leaves the list on the reader's behalf.
 *   Nothing of theirs reaches the sender.
 * - **A page** — their IP and user agent reach the sender the moment it
 *   loads, which is a decision to take deliberately. Offered as a link,
 *   never performed for them.
 * - **An address** — handed to the composer with the subject and body
 *   the sender asked for, because those are usually what it keys on.
 *
 * Pure, so the rule can be read in one place rather than inferred from
 * a chain of null checks inside a screen.
 */
sealed interface UnsubscribeOffer {

    data object OneClick : UnsubscribeOffer
    data class OpenPage(val url: String) : UnsubscribeOffer
    data class SendMail(val mailto: String) : UnsubscribeOffer
    data object None : UnsubscribeOffer

    /**
     * What the button says.
     *
     * Different words for different costs: the two that leave the app
     * say so, because a reader who taps "Unsubscribe" and lands in a
     * browser has been surprised.
     */
    val label: String
        get() = when (this) {
            OneClick -> "Unsubscribe"
            is OpenPage -> "Unsubscribe on the web"
            is SendMail -> "Unsubscribe by email"
            None -> ""
        }

    companion object {
        fun of(unsubscribe: Wire.Unsubscribe?): UnsubscribeOffer {
            if (unsubscribe == null) return None
            if (unsubscribe.oneClick) return OneClick
            // A page before an address: it is one tap against composing
            // and sending a message, and senders who offer both treat
            // them the same.
            unsubscribe.http.firstOrNull { it.isNotBlank() }?.let { return OpenPage(it) }
            unsubscribe.mailto.firstOrNull { it.isNotBlank() }?.let { return SendMail(it) }
            return None
        }
    }
}
