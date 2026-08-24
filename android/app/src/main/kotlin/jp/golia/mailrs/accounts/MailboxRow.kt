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
    /**
     * How big the whole message is, when the server said.
     *
     * On the row so a reader can be told what opening one would cost
     * before it costs it — a message with a 25 MB attachment is 25 MB
     * to fetch, and on mobile data that is a decision rather than a
     * tap.
     */
    val size: Long? = null,
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
     * How many rows one account may keep.
     *
     * **Raised from 2,000 when the rows moved into SQLite**, because
     * the old number was chosen for a cost that no longer exists. Every
     * row used to live in one preferences value, held in memory and
     * rewritten whole on every change, so an unbounded list made each
     * swipe-to-delete a rewrite of everything. A table addresses the
     * row that changed, and the limit now bounds disk instead.
     *
     * That mattered for more than tidiness: "load earlier" fetches
     * **older** mail, and a cap that keeps only the newest 2,000 threw
     * it away in the same pass that fetched it. On a mailbox with more
     * than 2,000 messages the button did nothing at all, and did it
     * slowly — which is not a shape any test with a three-message
     * script can see.
     *
     * **The gate on this number is now open.** It was 5,000 while the
     * list still read every row and sorted them in memory, because the
     * binding cost was that read and not the disk. The list reads a
     * window now (`ORDER BY … LIMIT`, `MailboxDatabase.newest`), so
     * what remains is disk: 50,000 is about 15 MB an account at
     * roughly 300 bytes a row, and bodies are not stored, so that is
     * the whole of it.
     *
     * 50,000 is also about a decade of a busy inbox — far enough above
     * what "load earlier" reaches that the ceiling is a real limit
     * rather than one somebody meets on a Tuesday.
     *
     * Choosing the limit means choosing what falls off. It is the
     * order the list itself uses, so what goes is exactly what somebody
     * would have had to scroll furthest to see.
     */
    const val PER_ACCOUNT = 50_000

    /**
     * Keep at most [limit] rows per account.
     *
     * **No production caller since the rows moved into SQLite** — the
     * table does its own capping, in SQL, because deciding what to drop
     * by loading everything is the cost that move was about. This stays
     * as the readable statement of the rule, and a test holds the SQL
     * to it. Delete both together or neither.
     *
     * **Per account, not overall.** One noisy mailbox would otherwise
     * evict a quiet one entirely, and the quiet one is where the mail
     * a person is waiting for tends to be.
     */
    fun capped(rows: List<MailboxRow>, limit: Int = PER_ACCOUNT): List<MailboxRow> {
        val byAccount = rows.groupBy { it.accountId }
        if (byAccount.values.none { it.size > limit }) return rows
        val keep = byAccount.values
            .flatMap { MailboxMerge.newestFirst(it).take(limit) }
            .map { it.id }
            .toSet()
        // Filtered rather than rebuilt from the groups, so the order
        // rows were held in survives — the list sorts them itself, and
        // reshuffling storage on every pass makes diffs unreadable.
        return rows.filter { it.id in keep }
    }
}
