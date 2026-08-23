package jp.golia.mailrs.accounts

import java.util.TimeZone

/**
 * An RFC 5322 message, ready to hand to a server.
 *
 * Pure: what goes on the wire is decided here and tested here, and
 * [SmtpSession] only carries it. A message that is wrong is wrong
 * whether or not there is a server to refuse it, and most servers do not
 * refuse — they deliver it looking broken.
 */
object OutgoingMessage {
    data class Draft(
        val from: String,
        val fromName: String = "",
        val to: List<String>,
        val cc: List<String> = emptyList(),
        val subject: String = "",
        val body: String = "",
        val inReplyTo: String = "",
        /**
         * The identities of the messages before this one, oldest first.
         * Without it a reply starts a new conversation in every client
         * that reads it, which is every client.
         */
        val references: List<String> = emptyList(),
    )

    /**
     * Everyone the message goes to.
     *
     * The envelope, not the headers: `Bcc` recipients belong here and
     * nowhere else, and a `Cc` recipient who is not in the envelope
     * never receives it however clearly the header names them.
     */
    fun envelope(draft: Draft, bcc: List<String> = emptyList()): List<String> {
        val seen = mutableSetOf<String>()
        val out = mutableListOf<String>()
        for (address in draft.to + draft.cc + bcc) {
            val trimmed = address.trim()
            if (trimmed.isEmpty()) continue
            // A duplicate recipient is a duplicate delivery: the same
            // person on To and Cc gets the message twice.
            if (!seen.add(trimmed.lowercase())) continue
            out.add(trimmed)
        }
        return out
    }

    /** The message itself. */
    fun text(draft: Draft, id: String, epochSeconds: Long, zone: TimeZone): String {
        val lines = mutableListOf<String>()
        lines.add("Message-ID: <$id>")
        lines.add("Date: " + MailDate.rfc5322(epochSeconds, zone))
        lines.add("From: " + address(draft.fromName, draft.from))
        if (draft.to.isNotEmpty()) lines.add("To: " + draft.to.joinToString(", "))
        if (draft.cc.isNotEmpty()) lines.add("Cc: " + draft.cc.joinToString(", "))
        // Never a Bcc header. It is in the envelope, and writing it here
        // is how a blind copy stops being blind.
        lines.add("Subject: " + EncodedWord.encode(draft.subject))
        if (draft.inReplyTo.isNotEmpty()) {
            lines.add("In-Reply-To: " + draft.inReplyTo)
            val chain = draft.references.toMutableList()
            if (draft.inReplyTo !in chain) chain.add(draft.inReplyTo)
            lines.add("References: " + chain.joinToString(" "))
        }
        lines.add("MIME-Version: 1.0")
        lines.add("Content-Type: text/plain; charset=utf-8")
        // 8bit, not base64: the body stays readable in every tool that
        // looks at a raw message, including the person debugging this.
        lines.add("Content-Transfer-Encoding: 8bit")
        lines.add("")
        return lines.joinToString("\r\n") + "\r\n" + normalised(draft.body)
    }

    /**
     * A display name, quoted only when it has to be.
     *
     * `Ada Lovelace <a@b>` is fine; `Lovelace, Ada <a@b>` is two
     * recipients to a parser that reads the comma, so a name with any of
     * the specials gets quoted.
     */
    fun address(name: String, email: String): String {
        val trimmed = name.trim()
        if (trimmed.isEmpty()) return email
        val encoded = EncodedWord.encode(trimmed)
        // An encoded word is already safe, and quoting one stops it
        // being decoded at all.
        if (encoded != trimmed) return "$encoded <$email>"
        val specials = "()<>[]:;@\\,.\""
        if (trimmed.any { it in specials }) {
            val escaped = trimmed.replace("\\", "\\\\").replace("\"", "\\\"")
            return "\"$escaped\" <$email>"
        }
        return "$trimmed <$email>"
    }

    /**
     * Every line ends CRLF.
     *
     * The dot-stuffing itself belongs to the session — it is a property
     * of the DATA command, not of the message — but the line endings are
     * the message's, and a body with bare newlines is what makes a
     * message arrive as one long line.
     */
    fun normalised(body: String): String {
        val unified = body.replace("\r\n", "\n").replace("\r", "\n")
        var text = unified.split("\n").joinToString("\r\n")
        if (!text.endsWith("\r\n")) text += "\r\n"
        return text
    }
}
