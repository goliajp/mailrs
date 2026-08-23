package jp.golia.mailrs.accounts

/**
 * What a reply starts out as.
 *
 * Pure, because every one of these decisions is a rule somebody can
 * disagree with, and a rule that can only be checked by sending mail is
 * a rule nobody checks.
 */
object ReplyDraft {
    /**
     * Reply to one message.
     *
     * - `Reply-To` wins over `From` — that is the entire purpose of the
     *   header, and ignoring it sends replies to a no-reply address.
     * - The subject gains one `Re:` and never a second.
     * - Threading is carried, or the reply starts a new conversation in
     *   every client that reads it.
     */
    fun make(
        headers: MessageHeaders.Parsed,
        account: MailAccount,
        quoting: String = "",
        /**
         * Copy everyone who was already on it.
         *
         * Off by default, and a separate button rather than a setting:
         * whether the rest of a list should see an answer is a decision
         * per message, and a client that decides it once decides it
         * wrong half the time.
         */
        all: Boolean = false,
    ): OutgoingMessage.Draft {
        val chain = headers.references.toMutableList()
        if (headers.messageId.isNotEmpty() && headers.messageId !in chain) {
            chain.add(headers.messageId)
        }
        val primary = recipient(headers)
        val copies = when {
            all -> MailAddresses.replyAll(headers.to, headers.cc, primary, account.address)
            else -> emptyList()
        }
        return OutgoingMessage.Draft(
            from = account.address,
            fromName = account.displayName,
            to = listOf(primary),
            cc = copies,
            subject = subject(headers.subject),
            body = quoted(quoting, headers),
            inReplyTo = headers.messageId,
            references = chain,
        )
    }

    fun recipient(headers: MessageHeaders.Parsed): String {
        val replyTo = headers.replyTo.trim()
        return when {
            replyTo.isNotEmpty() -> replyTo
            else -> headers.from
        }
    }

    /**
     * One `Re:`, never two.
     *
     * A conversation that has been round a few times otherwise reads
     * `Re: Re: Re: Re:`, and some clients thread on the subject.
     */
    fun subject(original: String): String {
        val trimmed = original.trim()
        if (trimmed.isEmpty()) return "Re:"
        var rest = trimmed
        // Strip every prefix that is already there, in the forms that
        // actually arrive — including the localised ones, which are what
        // a phone in Japan or China sends.
        var stripped = true
        while (stripped) {
            stripped = false
            for (prefix in listOf("re:", "re :", "答复:", "回复:", "回覆:")) {
                if (rest.lowercase().startsWith(prefix)) {
                    rest = rest.substring(prefix.length).trimStart()
                    stripped = true
                }
            }
        }
        return "Re: $rest"
    }

    /**
     * The original, marked as somebody else's words.
     *
     * Empty when there is nothing to quote: a reply that opens with an
     * attribution line above nothing looks like the message failed to
     * load.
     */
    fun quoted(body: String, headers: MessageHeaders.Parsed): String {
        val text = body.trim()
        if (text.isEmpty()) return ""
        val who = when {
            headers.from.isEmpty() -> "somebody"
            else -> headers.from
        }
        val lines = text.replace("\r\n", "\n").split("\n").map { line ->
            // No trailing space on an empty quoted line: it is invisible,
            // and it is what makes a quoted blank line show up as `> ` in
            // the reply.
            when {
                line.isEmpty() -> ">"
                else -> "> $line"
            }
        }
        return "\n\n$who wrote:\n" + lines.joinToString("\n") + "\n"
    }
}
