package jp.golia.mailrs.accounts

/**
 * Reading an address list out of a header.
 *
 * The whole of the difficulty is one character. `To:` is
 * comma-separated, and a display name may contain a comma — which is
 * why it is quoted. Splitting on every comma turns
 * `"Lovelace, Ada" <ada@example.com>` into two recipients, one of them
 * nonsense, and a reply-all then sends to an address that does not
 * exist.
 */
object MailAddresses {
    /** The entries of a header, still with their display names. */
    fun split(header: String): List<String> {
        val out = mutableListOf<String>()
        val current = StringBuilder()
        var quoted = false
        var angled = false
        for (ch in header) {
            when {
                ch == '"' -> {
                    quoted = !quoted
                    current.append(ch)
                }
                // A comma inside `<...>` is not a separator either: a
                // route address (`<@a,@b:c@d>`) is obsolete but legal,
                // and splitting one produces two broken halves.
                ch == '<' && !quoted -> {
                    angled = true
                    current.append(ch)
                }
                ch == '>' && !quoted -> {
                    angled = false
                    current.append(ch)
                }
                ch == ',' && !quoted && !angled -> {
                    out.add(current.toString().trim())
                    current.clear()
                }
                else -> current.append(ch)
            }
        }
        out.add(current.toString().trim())
        return out.filter { it.isNotEmpty() }
    }

    /**
     * The address itself, without the display name.
     *
     * For comparing, never for showing: `Ada <a@b>` and `a@b` are the
     * same person, and a reply-all that does not know it copies
     * somebody to their own message.
     */
    fun bare(entry: String): String {
        val open = entry.lastIndexOf('<')
        val close = entry.lastIndexOf('>')
        val inner = when {
            open in 0 until close -> entry.substring(open + 1, close)
            else -> entry
        }
        return inner.trim().lowercase()
    }

    /**
     * Everyone to copy on a reply-all, in the order they were written.
     *
     * Two rules, and both are about not annoying people: **the sender's
     * own address never appears**, or every reply-all copies its author,
     * and **nobody appears twice**, or somebody on both To and Cc gets
     * two.
     */
    fun replyAll(
        to: String,
        cc: String,
        primary: String,
        mine: String,
    ): List<String> {
        val skip = mutableSetOf(bare(primary), bare(mine))
        val out = mutableListOf<String>()
        for (entry in split(to) + split(cc)) {
            val key = bare(entry)
            if (key.isEmpty() || key in skip) continue
            skip.add(key)
            out.add(entry)
        }
        return out
    }
}
