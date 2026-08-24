package jp.golia.mailrs.accounts

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper

/**
 * Where the rows live.
 *
 * They used to be one JSON string in preferences, read whole and
 * **written whole on every change** — so a swipe-to-delete rewrote
 * every account's every row, and a person with six mailboxes reached
 * megabytes of that per tap. The cap that limits how many rows are
 * kept exists because of it, not because anybody wanted fewer rows.
 *
 * SQLite directly rather than Room: this is one table and five
 * statements, and a persistence framework would bring a build plugin
 * and a code generator to hold them.
 *
 * The key is `(account, folder, uid)` because that is what a row **is**
 * — a uid is unique within one folder of one account and nowhere else,
 * which is the same reason [MailboxRow.id] is spelled that way.
 */
class MailboxDatabase(context: Context, name: String = NAME) :
    SQLiteOpenHelper(context.applicationContext, name, null, VERSION) {

    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL(
            """
            CREATE TABLE rows (
                account TEXT NOT NULL,
                folder TEXT NOT NULL,
                uid INTEGER NOT NULL,
                seen INTEGER NOT NULL,
                sender TEXT NOT NULL,
                subject TEXT NOT NULL,
                date INTEGER,
                message_id TEXT NOT NULL,
                size INTEGER,
                haystack TEXT NOT NULL,
                PRIMARY KEY (account, folder, uid)
            )
            """.trimIndent(),
        )
        // The list reads newest-first across accounts, and the filter
        // reads one account at a time. Both are covered here so neither
        // walks the table.
        db.execSQL("CREATE INDEX rows_by_date ON rows (date DESC)")
        db.execSQL("CREATE INDEX rows_by_account ON rows (account)")
        // The unread badges are `WHERE seen = 0 GROUP BY account`, over
        // everything rather than a window — the one read here that is
        // not bounded by a LIMIT, so it is bounded by an index instead.
        db.execSQL("CREATE INDEX rows_by_seen ON rows (seen, account)")
    }

    override fun onUpgrade(db: SQLiteDatabase, from: Int, to: Int) {
        // Nothing to preserve yet: every row is a cache of what a
        // server has, and the next pass fetches it again. When that
        // stops being true this has to become a real migration.
        db.execSQL("DROP TABLE IF EXISTS rows")
        onCreate(db)
    }

    /** Every row, in no particular order — the list sorts them. */
    fun all(): List<MailboxRow> = readableDatabase.query(
        "rows", null, null, null, null, null, null,
    ).use { cursor ->
        buildList {
            while (cursor.moveToNext()) add(cursor.toRow())
        }
    }

    /**
     * The newest [limit] rows, in the order the list shows them.
     *
     * The list used to read **everything** and sort it in memory. That
     * is the read a table exists to avoid: the ordering is an index,
     * the window is a `LIMIT`, and neither grows with what the device
     * holds. It is also what lets the per-account cap rise — see
     * [MailboxApply.PER_ACCOUNT], whose number is set by this read.
     *
     * `accounts` is `null` for no filter at all, **not** the set of
     * every id: an empty set is a filter nothing satisfies, and
     * somebody who unticked every box should get an empty list rather
     * than the unfiltered one.
     */
    fun newest(limit: Int, accounts: Set<String>? = null): List<MailboxRow> =
        window(limit, accounts, emptyList())

    /**
     * The newest [limit] rows matching every word of [words].
     *
     * **Every** word, not any — somebody typing two words is
     * narrowing. The words may match different fields, so "ada lunch"
     * finds a message from Ada about lunch; the haystack is the same
     * one [MailboxSearch] builds, and a test holds the two to each
     * other rather than to a remembered spelling.
     */
    fun search(words: List<String>, limit: Int, accounts: Set<String>? = null) =
        window(limit, accounts, words)

    /** Unread, per account, over everything held — not over a window. */
    fun unreadPerAccount(): Map<String, Int> {
        val out = mutableMapOf<String, Int>()
        readableDatabase.rawQuery(
            "SELECT account, COUNT(*) FROM rows WHERE seen = 0 GROUP BY account", null,
        ).use { cursor ->
            while (cursor.moveToNext()) out[cursor.getString(0)] = cursor.getInt(1)
        }
        return out
    }

    /** Every folder this device holds something of, for one account. */
    fun folders(accountId: String): List<String> = readableDatabase.rawQuery(
        "SELECT DISTINCT folder FROM rows WHERE account = ?", arrayOf(accountId),
    ).use { cursor ->
        buildList { while (cursor.moveToNext()) add(cursor.getString(0)) }
    }

    /** How many rows one account holds. */
    fun count(accountId: String): Int = readableDatabase.rawQuery(
        "SELECT COUNT(*) FROM rows WHERE account = ?", arrayOf(accountId),
    ).use { cursor ->
        when {
            cursor.moveToFirst() -> cursor.getInt(0)
            else -> 0
        }
    }

    private fun window(
        limit: Int,
        accounts: Set<String>?,
        words: List<String>,
    ): List<MailboxRow> {
        val where = mutableListOf<String>()
        val args = mutableListOf<String>()
        if (accounts != null) {
            if (accounts.isEmpty()) return emptyList()
            where.add("account IN (${accounts.joinToString(",") { "?" }})")
            args.addAll(accounts)
        }
        for (word in words) {
            // Against a **stored** folded column, not `lower(...)` in
            // the query: SQLite's `lower` folds ASCII and nothing else,
            // so an accented subject would match here and not there —
            // divergence in exactly the alphabets nobody tests with.
            // The column is folded by [MailboxSearch.haystack], the
            // same function the in-memory search uses.
            where.add("haystack LIKE ?")
            args.add("%" + word.lowercase() + "%")
        }
        val clause = when {
            where.isEmpty() -> ""
            else -> "WHERE " + where.joinToString(" AND ")
        }
        args.add(limit.toString())
        return readableDatabase.rawQuery(
            """
            SELECT account, folder, uid, seen, sender, subject, date, message_id, size
            FROM rows $clause
            ORDER BY date IS NULL, date DESC,
                     account || '/' || folder || '/' || uid ASC
            LIMIT ?
            """.trimIndent(),
            args.toTypedArray(),
        ).use { cursor ->
            buildList { while (cursor.moveToNext()) add(cursor.toRow()) }
        }
    }

    /** Add or replace, in one transaction. */
    fun upsert(rows: List<MailboxRow>) {
        if (rows.isEmpty()) return
        val db = writableDatabase
        db.beginTransaction()
        try {
            for (row in rows) {
                db.insertWithOnConflict(
                    "rows", null, row.values(), SQLiteDatabase.CONFLICT_REPLACE,
                )
            }
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
    }

    /** Throw away everything and keep these — see AccountStore.replaceRows. */
    fun replaceAll(rows: List<MailboxRow>) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            db.delete("rows", null, null)
            for (row in rows) {
                db.insertWithOnConflict(
                    "rows", null, row.values(), SQLiteDatabase.CONFLICT_REPLACE,
                )
            }
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
    }

    fun deleteUids(accountId: String, folder: String, uids: Collection<Long>) {
        if (uids.isEmpty()) return
        val db = writableDatabase
        db.beginTransaction()
        try {
            for (uid in uids) {
                db.delete(
                    "rows", "account = ? AND folder = ? AND uid = ?",
                    arrayOf(accountId, folder, uid.toString()),
                )
            }
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
    }

    fun setUidsSeen(accountId: String, folder: String, flags: Map<Long, Boolean>) {
        if (flags.isEmpty()) return
        val db = writableDatabase
        db.beginTransaction()
        try {
            for ((uid, seen) in flags) {
                val values = ContentValues().apply { put("seen", if (seen) 1 else 0) }
                db.update(
                    "rows", values, "account = ? AND folder = ? AND uid = ?",
                    arrayOf(accountId, folder, uid.toString()),
                )
            }
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
    }

    /** One row, by the same identity the list uses. */
    fun delete(accountId: String, folder: String, uid: Long) {
        writableDatabase.delete(
            "rows", "account = ? AND folder = ? AND uid = ?",
            arrayOf(accountId, folder, uid.toString()),
        )
    }

    fun setSeen(accountId: String, folder: String, uid: Long, seen: Boolean) {
        val values = ContentValues().apply { put("seen", if (seen) 1 else 0) }
        writableDatabase.update(
            "rows", values, "account = ? AND folder = ? AND uid = ?",
            arrayOf(accountId, folder, uid.toString()),
        )
    }

    fun deleteAccount(accountId: String) {
        writableDatabase.delete("rows", "account = ?", arrayOf(accountId))
    }

    fun deleteFolder(accountId: String, folder: String) {
        writableDatabase.delete(
            "rows", "account = ? AND folder = ?", arrayOf(accountId, folder),
        )
    }

    /**
     * Keep at most [limit] rows for one account, newest first.
     *
     * **Per account**, because one noisy mailbox would otherwise evict
     * a quiet one entirely — and the quiet one is where the mail
     * somebody is waiting for tends to be.
     *
     * A row with no date sorts last, as it does on screen, so what
     * falls off is what somebody would have scrolled furthest to see.
     *
     * The ORDER BY is [MailboxMerge.newestFirst] spelled in SQL, down
     * to the tie-break on the row's id — arbitrary, but it has to be
     * the *same* arbitrary or the two disagree about which row falls
     * off, and only one of them is on screen. A test holds them to
     * each other rather than to a remembered ordering.
     */
    fun cap(accountId: String, limit: Int) {
        writableDatabase.execSQL(
            """
            DELETE FROM rows WHERE account = ? AND rowid NOT IN (
                SELECT rowid FROM rows WHERE account = ?
                ORDER BY date IS NULL, date DESC,
                         account || '/' || folder || '/' || uid ASC
                LIMIT ?
            )
            """.trimIndent(),
            arrayOf<Any>(accountId, accountId, limit),
        )
    }

    private fun MailboxRow.values(): ContentValues {
        val row = this
        return ContentValues().apply {
            put("account", accountId)
            put("folder", folder)
            put("uid", uid)
            put("seen", if (seen) 1 else 0)
            put("sender", sender)
            put("subject", subject)
            if (date == null) putNull("date") else put("date", date)
            put("message_id", messageId)
            if (size == null) putNull("size") else put("size", size)
            put("haystack", MailboxSearch.haystack(row))
        }
    }

    private fun android.database.Cursor.toRow() = MailboxRow(
        accountId = getString(getColumnIndexOrThrow("account")),
        uid = getLong(getColumnIndexOrThrow("uid")),
        folder = getString(getColumnIndexOrThrow("folder")),
        seen = getInt(getColumnIndexOrThrow("seen")) != 0,
        sender = getString(getColumnIndexOrThrow("sender")),
        subject = getString(getColumnIndexOrThrow("subject")),
        date = getColumnIndexOrThrow("date").let { if (isNull(it)) null else getLong(it) },
        messageId = getString(getColumnIndexOrThrow("message_id")),
        size = getColumnIndexOrThrow("size").let { if (isNull(it)) null else getLong(it) },
    )

    companion object {
        const val NAME = "mailboxes.db"
        const val VERSION = 2

        @Volatile
        private var shared: MailboxDatabase? = null

        /**
         * The one open connection for this process.
         *
         * Four screens each build their own [AccountStore], and a
         * helper per store is a *connection* per store. SQLite would
         * then arbitrate between them with file locks, and a write
         * arriving while another connection holds one comes back
         * `SQLITE_BUSY` — an error that appears only when two things
         * happen at once, which is to say on a real phone and not
         * here. One connection makes the arbitration in-process,
         * where it is a lock rather than a failure.
         *
         * Never closed: it lives as long as the process, and closing
         * it under a screen that still holds an [AccountStore] would
         * be the same defect from the other end.
         */
        fun shared(context: Context): MailboxDatabase =
            shared ?: synchronized(this) {
                shared ?: MailboxDatabase(context).also { shared = it }
            }
    }
}
