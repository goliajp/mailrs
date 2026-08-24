import Foundation
import SQLite3

/// Where the rows live.
///
/// They used to be one JSON blob in `UserDefaults`, read whole and
/// **written whole on every change** — so a swipe-to-delete rewrote
/// every account's every row, and a person with six mailboxes reached
/// megabytes of that per tap. The cap that limits how many rows are
/// kept exists because of it, not because anybody wanted fewer rows.
///
/// SQLite through its C interface rather than a wrapper package: this
/// is one table and a handful of statements, and a dependency would be
/// larger than what it carried.
///
/// The key is `(account, folder, uid)` because that is what a row
/// **is** — a uid is unique within one folder of one account and
/// nowhere else, which is the same reason `MailboxRow.id` is spelled
/// that way.
final class MailboxDatabase {
    /// SQLite's own "the string is mine now, copy it" marker, which the
    /// Swift overlay does not expose. Without it a bound string can be
    /// freed before the statement runs, and what lands in the row is
    /// whatever that memory became.
    private static let transient = unsafeBitCast(
        -1, to: sqlite3_destructor_type.self
    )

    private var handle: OpaquePointer?

    /// Opened with `FULLMUTEX`, so SQLite serialises access to this
    /// handle itself. That is what makes it safe to hand one instance
    /// to whatever thread a sync pass happens to be on, and why this
    /// type does not carry a lock of its own.
    init(path: String) throws {
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX
        guard sqlite3_open_v2(path, &handle, flags, nil) == SQLITE_OK else {
            let message = handle.map { String(cString: sqlite3_errmsg($0)) } ?? "could not open"
            sqlite3_close(handle)
            handle = nil
            throw Failure.sqlite(message)
        }
        // The rows are a cache of what a server has, so a schema that
        // has moved on is answered by starting again rather than by a
        // migration: the next pass fetches them. When that stops being
        // true this has to become a real migration.
        if try version() != MailboxDatabase.schema {
            try execute("DROP TABLE IF EXISTS rows")
            try execute("PRAGMA user_version = \(MailboxDatabase.schema)")
        }
        try execute(
            """
            CREATE TABLE IF NOT EXISTS rows (
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
            """
        )
        // The list reads newest-first across accounts, and the filter
        // reads one account at a time. Both are covered so neither
        // walks the table.
        try execute("CREATE INDEX IF NOT EXISTS rows_by_date ON rows (date DESC)")
        try execute("CREATE INDEX IF NOT EXISTS rows_by_account ON rows (account)")
        // The unread badges are `WHERE seen = 0 GROUP BY account`, over
        // everything rather than a window — the one read here that is
        // not bounded by a LIMIT, so it is bounded by an index instead.
        try execute("CREATE INDEX IF NOT EXISTS rows_by_seen ON rows (seen, account)")
    }

    deinit { sqlite3_close(handle) }

    /// What this file's shape is, so a build that changed it can say so.
    static let schema: Int32 = 2

    private func version() throws -> Int32 {
        var out: Int32 = 0
        try query("PRAGMA user_version") { statement in
            out = sqlite3_column_int(statement, 0)
        }
        return out
    }

    enum Failure: Error, Equatable {
        case sqlite(String)
    }

    // MARK: - reading

    /// Every row, in no particular order — the list sorts them.
    func all() throws -> [MailboxRow] {
        var out: [MailboxRow] = []
        // The columns are named rather than `*`, because the rest of
        // this file reads them back by position — and `*` makes that
        // position a property of the schema instead of a property of
        // this statement.
        try query(
            """
            SELECT account, folder, uid, seen, sender, subject, date, message_id, size
            FROM rows
            """
        ) { statement in
            out.append(MailboxDatabase.row(from: statement))
        }
        return out
    }

    /// The newest `limit` rows, in the order the list shows them.
    ///
    /// The list used to read **everything** and sort it in memory.
    /// That is the read a table exists to avoid: the ordering is an
    /// index, the window is a `LIMIT`, and neither grows with what the
    /// device holds. It is also what lets the per-account cap rise —
    /// see `MailboxApply.perAccount`, whose number this read sets.
    ///
    /// `accounts` is `nil` for no filter at all, **not** the set of
    /// every id: an empty set is a filter nothing satisfies, and
    /// somebody who unticked every box should get an empty list rather
    /// than the unfiltered one.
    func newest(limit: Int, accounts: Set<String>? = nil) throws -> [MailboxRow] {
        try window(limit: limit, accounts: accounts, words: [])
    }

    /// The newest `limit` rows matching every word of `words`.
    ///
    /// **Every** word, not any — somebody typing two words is
    /// narrowing. The words may match different fields, so "ada lunch"
    /// finds a message from Ada about lunch; the haystack is the one
    /// `MailboxSearch` builds, and a test holds the two to each other
    /// rather than to a remembered spelling.
    func search(words: [String], limit: Int, accounts: Set<String>? = nil) throws
        -> [MailboxRow]
    {
        try window(limit: limit, accounts: accounts, words: words)
    }

    /// Unread, per account, over everything held — not over a window.
    func unreadPerAccount() throws -> [String: Int] {
        var out: [String: Int] = [:]
        try query("SELECT account, COUNT(*) FROM rows WHERE seen = 0 GROUP BY account") {
            statement in
            guard let raw = sqlite3_column_text(statement, 0) else { return }
            out[String(cString: raw)] = Int(sqlite3_column_int64(statement, 1))
        }
        return out
    }

    /// Every folder this device holds something of, for one account.
    func folders(account: String) throws -> [String] {
        var out: [String] = []
        try prepared("SELECT DISTINCT folder FROM rows WHERE account = ?", [.text(account)]) {
            statement in
            while sqlite3_step(statement) == SQLITE_ROW {
                guard let raw = sqlite3_column_text(statement, 0) else { continue }
                out.append(String(cString: raw))
            }
        }
        return out
    }

    /// How many rows one account holds.
    func count(account: String) throws -> Int {
        var out = 0
        try prepared("SELECT COUNT(*) FROM rows WHERE account = ?", [.text(account)]) {
            statement in
            if sqlite3_step(statement) == SQLITE_ROW {
                out = Int(sqlite3_column_int64(statement, 0))
            }
        }
        return out
    }

    private func window(
        limit: Int, accounts: Set<String>?, words: [String]
    ) throws -> [MailboxRow] {
        var clauses: [String] = []
        var values: [Value] = []
        if let accounts {
            if accounts.isEmpty { return [] }
            clauses.append(
                "account IN (\(accounts.map { _ in "?" }.joined(separator: ",")))")
            values.append(contentsOf: accounts.map { Value.text($0) })
        }
        for word in words {
            // Against a **stored** folded column, not `lower(...)` in
            // the query: SQLite's `lower` folds ASCII and nothing else,
            // so an accented subject would match here and not there —
            // divergence in exactly the alphabets nobody tests with.
            // The column is folded by `MailboxSearch.haystack`, the
            // same function the in-memory search uses.
            clauses.append("haystack LIKE ?")
            values.append(.text("%" + word.lowercased() + "%"))
        }
        let filter = clauses.isEmpty ? "" : "WHERE " + clauses.joined(separator: " AND ")
        values.append(.int(Int64(limit)))
        var out: [MailboxRow] = []
        try prepared(
            """
            SELECT account, folder, uid, seen, sender, subject, date, message_id, size
            FROM rows \(filter)
            ORDER BY date IS NULL, date DESC,
                     account || '/' || folder || '/' || uid ASC
            LIMIT ?
            """, values
        ) { statement in
            while sqlite3_step(statement) == SQLITE_ROW {
                out.append(MailboxDatabase.row(from: statement))
            }
        }
        return out
    }

    // MARK: - writing

    /// Add or replace, in one transaction.
    func upsert(_ rows: [MailboxRow]) throws {
        guard !rows.isEmpty else { return }
        try transaction {
            for row in rows { try insert(row) }
        }
    }

    /// Throw away everything and keep these — see `AccountStore.replaceRows`.
    func replaceAll(_ rows: [MailboxRow]) throws {
        try transaction {
            try execute("DELETE FROM rows")
            for row in rows { try insert(row) }
        }
    }

    /// One row, by the same identity the list uses.
    func delete(account: String, folder: String, uid: UInt32) throws {
        try run(
            "DELETE FROM rows WHERE account = ? AND folder = ? AND uid = ?",
            [.text(account), .text(folder), .int(Int64(uid))]
        )
    }

    func delete(account: String, folder: String, uids: [UInt32]) throws {
        guard !uids.isEmpty else { return }
        try transaction {
            for uid in uids { try delete(account: account, folder: folder, uid: uid) }
        }
    }

    func setSeen(account: String, folder: String, uid: UInt32, seen: Bool) throws {
        try run(
            "UPDATE rows SET seen = ? WHERE account = ? AND folder = ? AND uid = ?",
            [.int(seen ? 1 : 0), .text(account), .text(folder), .int(Int64(uid))]
        )
    }

    func setSeen(account: String, folder: String, flags: [UInt32: Bool]) throws {
        guard !flags.isEmpty else { return }
        try transaction {
            for (uid, seen) in flags {
                try setSeen(account: account, folder: folder, uid: uid, seen: seen)
            }
        }
    }

    func deleteAccount(_ account: String) throws {
        try run("DELETE FROM rows WHERE account = ?", [.text(account)])
    }

    func deleteFolder(account: String, folder: String) throws {
        try run(
            "DELETE FROM rows WHERE account = ? AND folder = ?",
            [.text(account), .text(folder)]
        )
    }

    /// Keep at most `limit` rows for one account, newest first.
    ///
    /// **Per account**, because one noisy mailbox would otherwise evict
    /// a quiet one entirely — and the quiet one is where the mail
    /// somebody is waiting for tends to be.
    ///
    /// The ORDER BY is `MailboxApply.capped` spelled in SQL, down to
    /// the tie-break on the row's id — arbitrary, but it has to be the
    /// *same* arbitrary or the two disagree about which row falls off,
    /// and only one of them is on screen. A test holds them to each
    /// other rather than to a remembered ordering.
    func cap(account: String, limit: Int) throws {
        try run(
            """
            DELETE FROM rows WHERE account = ? AND rowid NOT IN (
                SELECT rowid FROM rows WHERE account = ?
                ORDER BY date IS NULL, date DESC,
                         account || '/' || folder || '/' || uid ASC
                LIMIT ?
            )
            """,
            [.text(account), .text(account), .int(Int64(limit))]
        )
    }

    // MARK: - the thin layer over the C interface

    private enum Value {
        case text(String)
        case int(Int64)
        case null
    }

    private func insert(_ row: MailboxRow) throws {
        try run(
            """
            INSERT OR REPLACE INTO rows
                (account, folder, uid, seen, sender, subject, date, message_id, size,
                 haystack)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            [
                .text(row.accountId), .text(row.folder), .int(Int64(row.uid)),
                .int(row.seen ? 1 : 0), .text(row.sender), .text(row.subject),
                row.date.map { Value.int($0) } ?? .null,
                .text(row.messageId),
                row.size.map { Value.int($0) } ?? .null,
                .text(MailboxSearch.haystack(of: row)),
            ]
        )
    }

    /// A transaction that rolls back on a throw.
    ///
    /// Without the rollback a failed pass would leave the table half
    /// updated — some rows of a folder replaced and some not — which is
    /// worse than not having written at all, because the next pass has
    /// no way to tell.
    private func transaction(_ body: () throws -> Void) throws {
        try execute("BEGIN")
        do {
            try body()
            try execute("COMMIT")
        } catch {
            try? execute("ROLLBACK")
            throw error
        }
    }

    private func execute(_ sql: String) throws {
        guard sqlite3_exec(handle, sql, nil, nil, nil) == SQLITE_OK else {
            throw Failure.sqlite(String(cString: sqlite3_errmsg(handle)))
        }
    }

    private func run(_ sql: String, _ values: [Value]) throws {
        try prepared(sql, values) { statement in
            let step = sqlite3_step(statement)
            guard step == SQLITE_DONE || step == SQLITE_ROW else {
                throw Failure.sqlite(String(cString: sqlite3_errmsg(handle)))
            }
        }
    }

    private func query(_ sql: String, each: (OpaquePointer) -> Void) throws {
        try prepared(sql, []) { statement in
            while sqlite3_step(statement) == SQLITE_ROW { each(statement) }
        }
    }

    private func prepared(
        _ sql: String, _ values: [Value], _ body: (OpaquePointer) throws -> Void
    ) throws {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(handle, sql, -1, &statement, nil) == SQLITE_OK,
              let statement
        else {
            throw Failure.sqlite(String(cString: sqlite3_errmsg(handle)))
        }
        defer { sqlite3_finalize(statement) }
        for (offset, value) in values.enumerated() {
            let index = Int32(offset + 1)
            switch value {
            case .text(let text):
                sqlite3_bind_text(statement, index, text, -1, MailboxDatabase.transient)
            case .int(let number):
                sqlite3_bind_int64(statement, index, number)
            case .null:
                sqlite3_bind_null(statement, index)
            }
        }
        try body(statement)
    }

    private static func row(from statement: OpaquePointer) -> MailboxRow {
        func text(_ index: Int32) -> String {
            guard let raw = sqlite3_column_text(statement, index) else { return "" }
            return String(cString: raw)
        }
        func number(_ index: Int32) -> Int64? {
            guard sqlite3_column_type(statement, index) != SQLITE_NULL else { return nil }
            return sqlite3_column_int64(statement, index)
        }
        return MailboxRow(
            accountId: text(0),
            uid: UInt32(truncatingIfNeeded: sqlite3_column_int64(statement, 2)),
            folder: text(1),
            seen: sqlite3_column_int64(statement, 3) != 0,
            sender: text(4),
            subject: text(5),
            date: number(6),
            messageId: text(7),
            size: number(8)
        )
    }
}
