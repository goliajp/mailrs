package jp.golia.mailrs.wire

import androidx.annotation.StringRes
import jp.golia.mailrs.R

/**
 * The sectioned Inbox's sections this app offers, as the server names them
 * (`crates/mailbox/src/section.rs`); the same set as the iPhone's `InboxSection`.
 */
enum class InboxSection(val wire: String, @StringRes val label: Int) {
    Reply("reply", R.string.section_reply),
    Awaiting("awaiting", R.string.section_awaiting),
    New("new", R.string.section_new),
    ;

    companion object {
        /**
         * The senders to accept from New: each row's last participant who is
         * not the reader, once each, lower-cased. The web's `lib/latest-senders.ts`.
         */
        fun latestSenders(rows: List<Wire.Conversation>, me: String): List<String> {
            val self = me.lowercase()
            val out = LinkedHashSet<String>()
            for (row in rows) {
                val last = row.participants.map(::bare).filter { it.isNotEmpty() && it != self }.lastOrNull()
                if (last != null) out += last
            }
            return out.toList()
        }

        private fun bare(shown: String): String {
            val open = shown.indexOf('<')
            val close = shown.lastIndexOf('>')
            if (open >= 0 && close > open) return shown.substring(open + 1, close).trim().lowercase()
            return shown.trim().lowercase()
        }
    }
}
