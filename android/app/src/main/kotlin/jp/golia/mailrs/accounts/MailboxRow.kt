package jp.golia.mailrs.accounts

import kotlinx.serialization.Serializable

/** One line in the list, from whichever mailbox it came. */
@Serializable
data class MailboxRow(
    /** Which account this arrived at. */
    val accountId: String,
    val uid: Long,
    val folder: String,
    val seen: Boolean,
    val sender: String,
    val subject: String,
    /** Seconds since the epoch, or null when the header was unreadable. */
    val date: Long?,
    /** The `Message-ID`, which is what survives a renumbering. */
    val messageId: String,
) {
    /**
     * Unique across accounts.
     *
     * A uid is unique **within one folder of one account** and nowhere
     * else: two accounts both have a message 1, and a list keyed on
     * uid alone shows one of them twice and the other never.
     */
    val id: String get() = "$accountId/$folder/$uid"

    /** What the row shows when nobody wrote a subject. */
    val displaySubject: String get() = subject.trim().ifEmpty { "(no subject)" }

    /**
     * What the row shows when the sender is missing altogether — a
     * blank line where a name goes reads as a rendering fault rather
     * than as an absent header.
     */
    val displaySender: String get() = sender.trim().ifEmpty { "(no sender)" }
}

/** Putting several mailboxes into one list. */
object MailboxMerge {
    /**
     * Newest first, and stable.
     *
     * Two messages can carry the same `Date:` — a mailing list fans
     * one message out to a hundred people in the same second — so the
     * sort ends on something that cannot tie. Without that the order
     * changes between two calls with the same input, and a list that
     * reorders itself while somebody reads it is worse than a wrong
     * order.
     *
     * A row with no readable date sorts **last**, not first: it is the
     * one thing this client knows nothing about, and putting it at the
     * top is the position that says "newest".
     */
    fun newestFirst(rows: List<MailboxRow>): List<MailboxRow> =
        rows.sortedWith(
            compareByDescending<MailboxRow> { it.date ?: Long.MIN_VALUE }.thenBy { it.id },
        )

    /**
     * Only these accounts, or all of them.
     *
     * `null` is no filter at all — not "every id", which selects the
     * same rows and says it less clearly. An **empty** set is a filter
     * nothing satisfies, and that distinction is the point: somebody
     * who unticked every box gets an empty list rather than the
     * unfiltered one.
     */
    fun onlyAccounts(rows: List<MailboxRow>, ids: Set<String>?): List<MailboxRow> =
        if (ids == null) rows else rows.filter { it.accountId in ids }

    /**
     * How old what is on screen is, across every account.
     *
     * **The oldest, not the newest**, and never a guess. With three
     * accounts where two synced a minute ago and one has been failing
     * since yesterday, "updated just now" is a lie about the third —
     * and the whole reason to show a time is to tell "no new mail"
     * apart from "we have not managed to check".
     *
     * `null` when any account has never synced at all, because then
     * some of the mail has never been fetched and no time describes
     * the screen.
     */
    fun oldestSync(accountIds: List<String>, lastSync: (String) -> Long?): Long? {
        if (accountIds.isEmpty()) return null
        var oldest = Long.MAX_VALUE
        for (id in accountIds) {
            val at = lastSync(id) ?: return null
            if (at < oldest) oldest = at
        }
        return oldest
    }

    /** How many of these are unread. */
    fun unreadCount(rows: List<MailboxRow>): Int = rows.count { !it.seen }

    /**
     * Unread per account, for the filter to say which is worth
     * looking at.
     *
     * **Accounts with none are absent from the map, not zero.** A
     * badge reading `0` is a badge that says nothing while taking up
     * the space of one that would, and every mail client hides it.
     */
    fun unreadPerAccount(rows: List<MailboxRow>): Map<String, Int> =
        rows.filterNot { it.seen }
            .groupingBy { it.accountId }
            .eachCount()
}

/** Folding a pass's worth of rows into what is already held. */
object MailboxApply {
    /**
     * The held rows, updated by what a pass just read.
     *
     * Matched on `id`, so a message read again is the **same row
     * updated** rather than a second copy. A pass that re-reads a
     * folder from the start — which is what a renumbering forces —
     * would otherwise double every message in the list.
     *
     * The **server's** flags win. It knows; this end is holding what
     * it knew last time, and a mailbox read on a phone and a laptop
     * disagrees within minutes otherwise.
     */
    fun apply(held: List<MailboxRow>, fetched: List<MailboxRow>): List<MailboxRow> {
        val byId = LinkedHashMap<String, MailboxRow>()
        for (row in held) byId[row.id] = row
        for (row in fetched) byId[row.id] = row
        return byId.values.toList()
    }

    /**
     * The rows of one folder replaced wholesale.
     *
     * For a renumbering: every uid held for that folder is a number
     * that no longer means anything, so keeping them beside the fresh
     * ones leaves a list of messages that cannot be opened.
     */
    fun replacingFolder(
        held: List<MailboxRow>,
        accountId: String,
        folder: String,
        fetched: List<MailboxRow>,
    ): List<MailboxRow> =
        held.filterNot { it.accountId == accountId && it.folder == folder } + fetched

    /**
     * Everything belonging to an account, gone.
     *
     * A row left behind when its account is removed is mail nobody can
     * open — the credential and the server it came from are both gone.
     */
    /**
     * Mark one row read, wherever it is in the list.
     *
     * The server was told; this is the same fact on this device, so the
     * list stops showing it as unread without waiting for the next
     * fetch.
     */
    fun markSeen(rows: List<MailboxRow>, id: String): List<MailboxRow> =
        rows.map { row ->
            when (row.id) {
                id -> row.copy(seen = true)
                else -> row
            }
        }

    fun withoutAccount(rows: List<MailboxRow>, accountId: String): List<MailboxRow> =
        rows.filterNot { it.accountId == accountId }
}
