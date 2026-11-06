package jp.golia.mailrs.wire

import androidx.annotation.StringRes
import jp.golia.mailrs.R

/**
 * The axes a list asks the server for.
 *
 * `ListQuery` in `crates/webapi/src/handlers/conversations.rs` takes
 * `folder`, `unread`, `starred`, `pinned`, `snoozed`, `archived`,
 * `trashed` and `label`; a list is a fixed combination of them and
 * nothing else. **Search takes the same axes**, which is why they live in
 * one type rather than as separate arguments — the list and the search
 * inside it must scope to the same thing, or searching Junk quietly
 * returns Inbox.
 *
 * Ported from `ios/Mailrs/Wire/MailList.swift`.
 */
data class MailListAxes(
    val folder: String? = null,
    val unread: Boolean? = null,
    val starred: Boolean? = null,
    val pinned: Boolean? = null,
    val snoozed: Boolean? = null,
    val archived: Boolean = false,
    val trashed: Boolean = false,
    /** One label's conversations: archived ones included, the Trash left out. */
    val label: String? = null,
    /**
     * The conversations held as suspected fraud. Not a predicate over
     * the other lists — it is the thing they leave out — so it is its
     * own endpoint, `/api/quarantine`, and the other axes do not apply.
     */
    val held: Boolean = false,
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
    /** One section of the sectioned inbox (`reply`, `awaiting`, `new`); null is the whole inbox. */
    val section: String? = null,
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
        pinned?.let { items += "pinned=$it" }
        snoozed?.let { items += "snoozed=$it" }
        if (trashed) items += "trashed=true"
        label?.let { items += "label=$it" }
        section?.let { items += "section=$it" }
        // Sent whenever the filter is narrowed — including to nothing,
        // which is the parameter present and empty. The server reads an
        // absent one as every account, so the two cannot be collapsed.
        accounts?.let { items += "accounts=" + it.joinToString(",") }
        // Review is listed from its own endpoint; a search typed into it
        // names it with this flag, or it would search every other list
        if (held) items += "quarantined=true"
        return items.joinToString("&")
    }
}

/**
 * The lists this app shows, and what each one is.
 *
 * The same set the web declares in `lib/mail-lists.ts` and iOS in
 * `MailList.swift`, minus Send and Draft — those read different
 * endpoints entirely and are not folders — and in the web's order.
 * Keeping the axes here rather than at the call sites is what stops
 * "which threads is Starred" from being answered differently by the
 * list, the search and the unread count.
 *
 * [Label] is one entry for every label: the label itself is chosen
 * beside it ([labelAxes]), because labels are the reader's own and not
 * known when this is compiled.
 */
enum class MailList(
    @StringRes val title: Int,
    @StringRes val emptyMessage: Int,
    val axes: MailListAxes,
) {
    Inbox(R.string.list_inbox, R.string.list_inbox_empty, MailListAxes(folder = "Inbox")),

    /** The server merges Notifications and Promotions; `NP` is the name its folder parser knows. */
    NP(R.string.list_np, R.string.list_np_empty, MailListAxes(folder = "NP")),

    /**
     * `NonJunk`, not null: unread and starred are attributes of a thread
     * rather than places one lives, and scoping them to everything would
     * drag Junk back out of the one surface it is allowed to have.
     */
    Unread(R.string.list_unread, R.string.list_unread_empty, MailListAxes(folder = "NonJunk", unread = true)),

    Starred(R.string.list_starred, R.string.list_starred_empty, MailListAxes(folder = "NonJunk", starred = true)),

    Pinned(R.string.list_pinned, R.string.list_pinned_empty, MailListAxes(folder = "NonJunk", pinned = true)),

    /** Put away until a time still to come; they come back to the inbox by themselves. */
    Snoozed(R.string.list_snoozed, R.string.list_snoozed_empty, MailListAxes(snoozed = true)),

    /** Everything but Junk and the Trash, archived mail included. */
    All(R.string.list_all, R.string.list_all_empty, MailListAxes(folder = "all")),

    /**
     * No folder. Archived is cross-folder — the server drops the folder
     * when this is set, because "archived within Inbox" is not what the
     * list means.
     */
    Archived(R.string.list_archived, R.string.list_archived_empty, MailListAxes(archived = true)),

    Junk(R.string.list_junk, R.string.list_junk_empty, MailListAxes(folder = "Junk")),

    Review(R.string.list_review, R.string.list_review_empty, MailListAxes(held = true)),

    Trash(R.string.list_trash, R.string.list_trash_empty, MailListAxes(trashed = true)),

    Label(R.string.list_label, R.string.list_label_empty, MailListAxes()),
    ;

    companion object {
        /** The lists the drawer offers by name; labels are listed under them. */
        val fixed: List<MailList> = entries.filter { it != Label }

        /** One label's list. */
        fun labelAxes(labelId: String) = MailListAxes(label = labelId)

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
