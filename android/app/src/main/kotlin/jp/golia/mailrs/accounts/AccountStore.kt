package jp.golia.mailrs.accounts

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json

/**
 * The accounts this person has added, and their secrets.
 *
 * Two stores, deliberately apart. The **rows** are ordinary
 * preferences — they hold no secret and a person may want to see them.
 * The **credentials** are encrypted with a key the app never sees,
 * which is what makes "delete the account" also mean "the password is
 * gone" rather than leaving a secret nobody can see and nobody will
 * remove.
 *
 * The same shape as `TokenStore`, and for the reason recorded there:
 * `EncryptedSharedPreferences` is deprecated as of security-crypto
 * 1.1.0, so this does the thing it wrapped — an AES-GCM key in the
 * Android Keystore and the ciphertext in ordinary private
 * preferences. The key material stays in the TEE, so a copied prefs
 * file is useless.
 *
 * Its own key alias, not the session's: clearing one must not make the
 * other undecryptable, and a mail password outlives a session by
 * design.
 */
class AccountStore(context: Context) {
    private val prefs = context.applicationContext
        .getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val json = Json { ignoreUnknownKeys = true }
    private val db = MailboxDatabase.shared(context)

    fun load(): List<MailAccount> {
        val raw = prefs.getString(ROWS, null) ?: return emptyList()
        return runCatching {
            json.decodeFromString(ListSerializer(MailAccount.serializer()), raw)
        }.getOrDefault(emptyList()).sortedBy { it.sort }
    }

    fun save(accounts: List<MailAccount>) {
        prefs.edit()
            .putString(ROWS, json.encodeToString(ListSerializer(MailAccount.serializer()), accounts))
            .apply()
    }

    /** Add or replace one, keeping the list in order. */
    fun upsert(account: MailAccount) {
        save(load().filterNot { it.id == account.id } + account)
    }

    /**
     * Remove one **and its secret**.
     *
     * Both, always: a row removed while its stored credential stays
     * behind is a secret nobody can see and nobody will delete.
     */
    fun remove(id: String) {
        save(load().filterNot { it.id == id })
        prefs.edit().remove(secretKeyName(id)).apply()
        // And the connection that credential signed in: a socket
        // left open is still authenticated as somebody who has just
        // been removed, and the next tap would reuse it.
        ImapPool.shared.drop(id)
        // And its mail, and where each of its folders was left. A row
        // left behind is mail nobody can open — the credential and the
        // server it came from are both gone — and a mark left behind
        // makes the next account with the same address resume from
        // somebody else's place.
        val prefix = "$id/"
        saveMarks(marks().filterKeys { !it.startsWith(prefix) })
        prefs.edit().remove(POP_SEEN + id).remove(LAST_SYNC + id).apply()
        migrateRowsOnce()
        db.deleteAccount(id)
    }

    // MARK: the mail itself

    /**
     * Every row from every connected mailbox.
     *
     * Ordinary preferences, not the encrypted store: these are
     * headers, and a person can already see them on screen. The
     * **bodies** are not stored at all — they are fetched when a
     * message is opened, so nothing here grows without bound and
     * nothing here is worth stealing.
     */
    fun rows(): List<MailboxRow> {
        migrateRowsOnce()
        return db.all()
    }

    /**
     * Throw away every row and keep these instead.
     *
     * Named for what it does. It is what a test that wants a known
     * starting point needs, and what nothing on a sync path should
     * use — [upsertRows], [deleteRow] and [setRowSeen] address the
     * rows that actually changed, which is the whole reason the rows
     * moved out of one preferences string.
     */
    fun replaceRows(rows: List<MailboxRow>) {
        migrateRowsOnce()
        db.replaceAll(rows)
    }

    /** Add or update, leaving every other row alone. */
    fun upsertRows(rows: List<MailboxRow>) {
        migrateRowsOnce()
        db.upsert(rows)
    }

    fun deleteRow(row: MailboxRow) {
        migrateRowsOnce()
        db.delete(row.accountId, row.folder, row.uid)
    }

    fun deleteUids(accountId: String, folder: String, uids: Collection<Long>) {
        migrateRowsOnce()
        db.deleteUids(accountId, folder, uids)
    }

    fun setRowSeen(row: MailboxRow, seen: Boolean) {
        migrateRowsOnce()
        db.setSeen(row.accountId, row.folder, row.uid, seen)
    }

    fun setUidsSeen(accountId: String, folder: String, flags: Map<Long, Boolean>) {
        migrateRowsOnce()
        db.setUidsSeen(accountId, folder, flags)
    }

    fun dropFolder(accountId: String, folder: String) {
        migrateRowsOnce()
        db.deleteFolder(accountId, folder)
    }

    /** The newest rows, in the order the list shows them. */
    fun newest(limit: Int, accounts: Set<String>? = null): List<MailboxRow> {
        migrateRowsOnce()
        return db.newest(limit, accounts)
    }

    /** The newest rows matching every word. */
    fun search(words: List<String>, limit: Int, accounts: Set<String>? = null): List<MailboxRow> {
        migrateRowsOnce()
        return db.search(words, limit, accounts)
    }

    /** Unread per account, over everything held rather than a window. */
    fun unreadPerAccount(): Map<String, Int> {
        migrateRowsOnce()
        return db.unreadPerAccount()
    }

    /**
     * How many rows one account holds.
     *
     * A `COUNT(*)`, not a filter over every row: the one caller is the
     * ceiling check on the "load earlier" path, and loading the table
     * to decide whether the table is full is the read this whole layer
     * exists to remove.
     */
    fun count(accountId: String): Int {
        migrateRowsOnce()
        return db.count(accountId)
    }

    /** Every folder this device holds something of, for one account. */
    fun folders(accountId: String): List<String> {
        migrateRowsOnce()
        return db.folders(accountId)
    }

    fun capAccount(accountId: String, limit: Int = MailboxApply.PER_ACCOUNT) {
        migrateRowsOnce()
        db.cap(accountId, limit)
    }

    /**
     * Move whatever the preferences blob still holds into the table.
     *
     * Runs at most once per install: a device upgrading from a build
     * that kept its rows as one JSON string would otherwise show an
     * empty list until the next sync, which reads as lost mail. The
     * key is removed afterwards so a later downgrade-and-upgrade
     * cannot resurrect rows the person has since deleted.
     */
    private fun migrateRowsOnce() {
        val raw = prefs.getString(ROWS_MAIL, null) ?: return
        val carried = runCatching {
            json.decodeFromString(ListSerializer(MailboxRow.serializer()), raw)
        }.getOrDefault(emptyList())
        db.upsert(carried)
        prefs.edit().remove(ROWS_MAIL).apply()
    }

    /**
     * Where each folder of each account was left.
     *
     * Keyed `accountId/folder`, so two accounts with an INBOX each
     * keep their own place — the mistake this key shape prevents is
     * the same one [MailboxRow.id] prevents in the list.
     */
    fun marks(): Map<String, FolderMark> {
        val raw = prefs.getString(MARKS, null) ?: return emptyMap()
        return runCatching {
            json.decodeFromString(
                MapSerializer(String.serializer(), FolderMark.serializer()),
                raw,
            )
        }.getOrDefault(emptyMap())
    }

    fun saveMarks(marks: Map<String, FolderMark>) {
        prefs.edit()
            .putString(
                MARKS,
                json.encodeToString(
                    MapSerializer(String.serializer(), FolderMark.serializer()),
                    marks,
                ),
            )
            .apply()
    }

    /** The marks of one account, without its prefix. */
    fun marksFor(accountId: String): Map<String, FolderMark> {
        val prefix = "$accountId/"
        return marks().filterKeys { it.startsWith(prefix) }
            .mapKeys { it.key.removePrefix(prefix) }
    }

    /**
     * Store one account's marks back, leaving every other account's
     * alone.
     *
     * A **replacement** of that account's set rather than a merge into
     * it: a folder that has been renamed or removed would otherwise
     * keep its old place forever.
     */
    fun saveMarksFor(accountId: String, folderMarks: Map<String, FolderMark>) {
        val prefix = "$accountId/"
        val all = marks().filterKeys { !it.startsWith(prefix) }.toMutableMap()
        for ((folder, mark) in folderMarks) all[prefix + folder] = mark
        saveMarks(all)
    }

    /**
     * The uidls a POP3 account has already read.
     *
     * Not [FolderMark]: POP3 has no folders and no uid validity, and
     * its message numbers are renumbered every session. The uidl is the
     * only durable identity it offers, so what is remembered is a set of
     * them — pruned each pass to what the server still has, or the
     * bookkeeping outgrows the mailbox.
     */
    fun popSeen(accountId: String): Set<String> {
        val raw = prefs.getString(POP_SEEN + accountId, null) ?: return emptySet()
        return runCatching {
            json.decodeFromString(ListSerializer(String.serializer()), raw).toSet()
        }.getOrDefault(emptySet())
    }

    fun savePopSeen(accountId: String, ids: Set<String>) {
        prefs.edit()
            .putString(
                POP_SEEN + accountId,
                json.encodeToString(ListSerializer(String.serializer()), ids.toList()),
            )
            .apply()
    }

    /**
     * When each account was last read successfully.
     *
     * Kept so the list can say how old what it is showing is. "No new
     * mail" and "we have not managed to check since yesterday" look
     * identical on screen, and only one of them is a reason to relax.
     */
    fun lastSync(accountId: String): Long? {
        val at = prefs.getLong(LAST_SYNC + accountId, 0L)
        return when (at) {
            0L -> null
            else -> at
        }
    }

    fun saveLastSync(accountId: String, epochSeconds: Long) {
        prefs.edit().putLong(LAST_SYNC + accountId, epochSeconds).apply()
    }

    fun saveSecret(secret: String, id: String) {
        prefs.edit().putString(secretKeyName(id), encrypt(secret)).apply()
    }

    /**
     * The secret, or null.
     *
     * Null covers both "there is none" and "the key is gone" — the
     * latter happens when app data is cleared, and it reads as "sign
     * in again", which is the truthful outcome.
     */
    fun secret(id: String): String? =
        prefs.getString(secretKeyName(id), null)?.let { decrypt(it) }

    private fun secretKeyName(id: String) = "secret.$id"

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (ks.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return generator.generateKey()
    }

    /** `base64(iv) : base64(ciphertext)`. The IV is not a secret. */
    private fun encrypt(plain: String): String {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val body = cipher.doFinal(plain.toByteArray())
        return b64(cipher.iv) + ":" + b64(body)
    }

    private fun decrypt(blob: String): String? {
        val parts = blob.split(":")
        if (parts.size != 2) return null
        return runCatching {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(
                Cipher.DECRYPT_MODE,
                key(),
                GCMParameterSpec(TAG_BITS, unb64(parts[0])),
            )
            String(cipher.doFinal(unb64(parts[1])))
        }.getOrNull()
    }

    private fun b64(bytes: ByteArray) =
        android.util.Base64.encodeToString(bytes, android.util.Base64.NO_WRAP)

    private fun unb64(s: String) =
        android.util.Base64.decode(s, android.util.Base64.NO_WRAP)

    private companion object {
        const val PREFS = "mailrs.accounts"
        const val ROWS = "rows.v1"
        const val ROWS_MAIL = "mailbox.rows.v1"
        const val MARKS = "mailbox.marks.v1"
        const val POP_SEEN = "pop.seen.v1."
        const val LAST_SYNC = "last.sync.v1."
        const val KEYSTORE = "AndroidKeyStore"
        const val KEY_ALIAS = "mailrs.account.secret.v1"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val TAG_BITS = 128
    }
}
