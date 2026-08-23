package jp.golia.mailrs.accounts

/**
 * Which folder a deleted message goes to.
 *
 * There is no single answer, which is the whole point of this file.
 * Gmail calls it `[Gmail]/Trash`, Outlook `Deleted Items`, iCloud
 * `Deleted Messages`, and a server in Japan may call it `ゴミ箱`. A
 * client that hard-codes `Trash` deletes nothing on three of those and
 * **creates a folder called Trash** on some of them, where the message
 * then sits invisible to every other client the person uses.
 */
object TrashFolder {
    /**
     * The `\Trash` attribute (RFC 6154) is the server saying it in so
     * many words, and it is right regardless of language. It is asked
     * first for that reason.
     */
    private const val SPECIAL_USE = "\\TRASH"

    /**
     * Names to fall back on, for servers that do not publish
     * special-use. Matched case-insensitively and by the last path
     * segment, so `[Gmail]/Trash` and `INBOX.Trash` both count.
     */
    private val KNOWN = listOf(
        "trash", "deleted items", "deleted messages", "bin", "ゴミ箱", "已删除邮件", "垃圾桶",
    )

    /**
     * @param folders every folder the server listed, with attributes.
     * @return the folder to move to, or null — and **null means do not
     *   delete**. Guessing a name and having the server create it puts
     *   the message somewhere no other client will look.
     */
    fun pick(folders: List<Imap.Untagged.ListFolder>): String? {
        folders.firstOrNull { f -> f.attributes.any { it.uppercase() == SPECIAL_USE } }
            ?.let { return it.name }
        for (name in KNOWN) {
            folders.firstOrNull { lastSegment(it.name).lowercase() == name }?.let { return it.name }
        }
        return null
    }

    /** `[Gmail]/Trash` and `INBOX.Trash` are both called Trash. */
    private fun lastSegment(name: String): String =
        name.split('/', '.').last { it.isNotEmpty() }
}
