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
        /** What to send with it. */
        val attachments: List<Attachment> = emptyList(),
    )

    /** A file on its way out. */
    data class Attachment(
        val filename: String,
        val mimeType: String,
        val bytes: ByteArray,
    ) {
        // Generated equals would compare the array by identity, which
        // makes two reads of the same file unequal.
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is Attachment) return false
            return filename == other.filename && mimeType == other.mimeType &&
                bytes.contentEquals(other.bytes)
        }

        override fun hashCode(): Int =
            filename.hashCode() * 31 + mimeType.hashCode() * 31 + bytes.contentHashCode()
    }

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
        if (draft.attachments.isEmpty()) {
            lines.add("Content-Type: text/plain; charset=utf-8")
            // 8bit, not base64: the body stays readable in every tool
            // that looks at a raw message, including the person
            // debugging this.
            lines.add("Content-Transfer-Encoding: 8bit")
            lines.add("")
            return lines.joinToString("\r\n") + "\r\n" + normalised(draft.body)
        }
        val boundary = boundary(draft, id)
        lines.add("Content-Type: multipart/mixed; boundary=\"$boundary\"")
        lines.add("")
        val parts = StringBuilder()
        // The text first, always. Every reader shows the first text
        // part it finds, and a message whose first part is a PDF opens
        // as a PDF with the words underneath it.
        parts.append("--").append(boundary).append("\r\n")
        parts.append("Content-Type: text/plain; charset=utf-8\r\n")
        parts.append("Content-Transfer-Encoding: 8bit\r\n\r\n")
        parts.append(normalised(draft.body))
        for (attachment in draft.attachments) {
            parts.append("--").append(boundary).append("\r\n")
            parts.append("Content-Type: ").append(attachment.mimeType)
            // The name in both places: `Content-Type: name=` is the
            // older spelling and some readers still look only there.
            parts.append("; name=\"").append(headerSafe(attachment.filename)).append("\"\r\n")
            parts.append("Content-Disposition: attachment; filename=\"")
                .append(headerSafe(attachment.filename)).append("\"\r\n")
            parts.append("Content-Transfer-Encoding: base64\r\n\r\n")
            parts.append(wrapped(java.util.Base64.getEncoder().encodeToString(attachment.bytes)))
        }
        parts.append("--").append(boundary).append("--\r\n")
        return lines.joinToString("\r\n") + "\r\n" + parts
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
     * A boundary that cannot appear in the message.
     *
     * Derived from the message id, which is already unique, and
     * padded with characters no encoder emits — a boundary that turns
     * up inside a part cuts the message in half at that point.
     */
    private fun boundary(draft: Draft, id: String): String =
        "----=_mailrs_" + id.filter { it.isLetterOrDigit() }.take(24) + "_" +
            draft.attachments.size

    /**
     * A filename that cannot break the header it sits in.
     *
     * Quotes and backslashes end the quoted string early, and a
     * newline ends the header — which is how a filename becomes an
     * injected header. RFC 2231 would encode a non-ASCII name; this
     * keeps it as UTF-8, which every current reader accepts and which
     * is what the alternative degrades to anyway.
     */
    private fun headerSafe(name: String): String =
        name.replace("\\", "").replace("\"", "").replace("\r", "").replace("\n", "")

    /** Base64 at 76 characters, as RFC 2045 asks. */
    private fun wrapped(text: String): String =
        text.chunked(76).joinToString("\r\n") + "\r\n"

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
