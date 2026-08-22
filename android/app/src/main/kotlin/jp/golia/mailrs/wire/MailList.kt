package jp.golia.mailrs.wire

/**
 * The axes a list asks the server for.
 *
 * `ListQuery` in `crates/webapi/src/handlers/conversations.rs` takes
 * `folder`, `unread`, `starred` and `archived`; a list is a fixed
 * combination of them and nothing else. **Search takes the same four**,
 * which is why they live in one type rather than as separate arguments —
 * the list and the search inside it must scope to the same thing, or
 * searching Junk quietly returns Inbox.
 *
 * Ported from `ios/Mailrs/Wire/MailList.swift`.
 */
data class MailListAxes(
    val folder: String? = null,
    val unread: Boolean? = null,
    val starred: Boolean? = null,
    val archived: Boolean = false,
    /**
     * Which connected mailboxes to narrow to.
     *
     * `null` is every account, and it is not the same as an empty
     * list: somebody who has unticked every box is asking for nothing,
     * and answering with everything would answer a question they did
     * not ask. This server's own mail is the empty string, so it can
     * be switched off like any other.
     */
    val accounts: List<String>? = null,
) {
    /**
     * The query string both `/api/conversations` and its `/search` take.
     *
     * Absent rather than false for the optional flags: `unread=false`
     * asks for threads that are *read*, which is a different list from
     * "not filtered by unread". `archived` is the exception — the
     * handler declares it `#[serde(default)]` and always means a
     * boolean, so it is always sent.
     */
    fun query(): String {
        val items = mutableListOf("archived=$archived")
        folder?.let { items += "folder=$it" }
        unread?.let { items += "unread=$it" }
        starred?.let { items += "starred=$it" }
        // Sent whenever the filter is narrowed — including to nothing,
        // which is the parameter present and empty. The server reads an
        // absent one as every account, so the two cannot be collapsed.
        accounts?.let { items += "accounts=" + it.joinToString(",") }
        return items.joinToString("&")
    }
}

/**
 * The lists this app shows, and what each one is.
 *
 * The same set the web declares in `lib/mail-lists.ts` and iOS in
 * `MailList.swift`, minus Send — that reads different endpoints
 * entirely and is not a folder. Keeping the axes here rather than at
 * the call sites is what stops "which threads is Starred" from being
 * answered differently by the list, the search and the unread count.
 */
enum class MailList(val title: String, val emptyMessage: String, val axes: MailListAxes) {
    Inbox("Inbox", "All caught up", MailListAxes(folder = "Inbox")),

    /** The server merges Notifications and Promotions; `NP` is the name its folder parser knows. */
    NP("N & P", "Nothing here", MailListAxes(folder = "NP")),

    /**
     * `NonJunk`, not null: unread and starred are attributes of a thread
     * rather than places one lives, and scoping them to everything would
     * drag Junk back out of the one surface it is allowed to have.
     */
    Unread("Unread", "All caught up", MailListAxes(folder = "NonJunk", unread = true)),

    Starred("Starred", "Nothing starred", MailListAxes(folder = "NonJunk", starred = true)),

    Junk("Junk", "No junk mail", MailListAxes(folder = "Junk")),

    /**
     * No folder. Archived is cross-folder — the server drops the folder
     * when this is set, because "archived within Inbox" is not what the
     * list means.
     */
    Archived("Archived", "No archived conversations", MailListAxes(archived = true)),
    ;

    companion object {
        /**
         * A list named by something other than this enum.
         *
         * Only the instrumented suite uses it, for the stub's `Paged`
         * fixture — 120 threads with a deliberate timestamp collision at
         * rows 48-52, which is the only way to exercise keyset paging
         * against a real server-shaped answer.
         */
        fun named(folder: String) = MailListAxes(folder = folder)
    }
}
