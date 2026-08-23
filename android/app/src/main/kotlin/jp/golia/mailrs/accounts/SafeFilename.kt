package jp.golia.mailrs.accounts

/**
 * A name from a stranger, made safe to write to disk.
 *
 * **An attachment's filename is attacker-controlled.** It arrives in a
 * header written by whoever sent the message, and a client that hands
 * it to the filesystem unchanged can be told to write
 * `../../../shared_prefs/auth.xml`. This has been a real bug in real
 * mail clients more than once; it is not a hypothetical.
 *
 * Not a general "make this string safe" function: mapping every
 * awkward character to `-` is right for a cache key nobody reads and
 * wrong here, because `report 2025.pdf` would become `report-2025-pdf`
 * and a file with no extension is a file the phone will not open. A
 * name a person sees has to survive.
 *
 * The rule is simple and total: **the result is one path segment, or it
 * is the fallback.** What matters is the property — nothing that comes
 * out of here can escape the directory it is written into — and the
 * tests assert that rather than any particular string, so the rule can
 * change without the guarantee doing so.
 */
object SafeFilename {
    /** Longer than most filesystems accept, and every one truncates. */
    private const val MAX_BYTES = 200

    fun of(name: String, fallback: String = "attachment"): String {
        // The **leaf**, in either separator: `../../x.plist` is
        // `x.plist`, which is the name the sender meant and cannot
        // escape the directory it is written into. Refusing outright
        // would be safe too and would throw a usable name away — and
        // iOS already collapses, so refusing here would make the two
        // clients disagree about what a file is called.
        val trimmed = name.trim().substringAfterLast('/').substringAfterLast('\\').trim()
        if (trimmed == "." || trimmed == "..") return fallback
        // Control characters, including the NUL that truncates a name in
        // every C API underneath.
        val clean = trimmed.filter { it.code >= 0x20 && it.code != 0x7F }
        if (clean.isEmpty()) return fallback
        // A leading dot is **kept**. It was tempting to strip it — a
        // dotfile hides from a file manager — but this file goes to the
        // cache and straight to another app through a content URI,
        // where nobody browses for it. Stripping would rename a
        // `.gitignore` somebody attached on purpose, and renaming what
        // a person sent is worse than a name that would have been
        // hidden somewhere this file never goes.
        return truncated(clean, fallback)
    }

    /**
     * Shortened from the **stem**, never from the end.
     *
     * A name cut at the end loses its extension, and a file the phone
     * cannot tell the type of is a file nothing will open.
     */
    private fun truncated(name: String, fallback: String): String {
        if (name.toByteArray(Charsets.UTF_8).size <= MAX_BYTES) return name
        val dot = name.lastIndexOf('.')
        val extension = when {
            dot > 0 && name.length - dot <= 12 -> name.substring(dot)
            else -> ""
        }
        val room = MAX_BYTES - extension.toByteArray(Charsets.UTF_8).size
        if (room <= 0) return fallback
        val stem = name.substring(0, name.length - extension.length)
        // By bytes, and never through a character: a name cut inside a
        // multi-byte sequence writes a filename with a replacement
        // character in it.
        val out = StringBuilder()
        var used = 0
        for (ch in stem) {
            val size = ch.toString().toByteArray(Charsets.UTF_8).size
            if (used + size > room) break
            out.append(ch)
            used += size
        }
        if (out.isEmpty()) return fallback
        return out.toString() + extension
    }
}
