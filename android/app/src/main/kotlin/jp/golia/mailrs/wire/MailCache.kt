package jp.golia.mailrs.wire

import android.content.Context
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import java.io.File

/**
 * Last-known mail, on disk, so a cold launch opens on the mailbox
 * rather than a spinner — and so a phone on a train shows the mail it
 * already had.
 *
 * Ported from `ios/Mailrs/App/MailCache.swift`, including the rule that
 * makes it safe: **strictly a display cache**. Every successful fetch
 * overwrites it, a missing or unreadable file answers null and the
 * caller fetches as if it never existed, and nothing here is a source of
 * truth. Corruption is handled by deletion, not repair.
 *
 * On a phone this matters more than on a laptop: the network goes away
 * mid-journey, and an app that answers "Could not load your mail" to
 * somebody holding a mailbox it fetched two minutes ago has thrown away
 * what it had.
 */
class MailCache(context: Context, directory: File? = null) {

    private val dir = directory ?: File(context.cacheDir, "mail-cache")

    private val json = Json { ignoreUnknownKeys = true }

    fun readConversations(list: String): List<Wire.Conversation>? =
        read(ListSerializer(Wire.Conversation.serializer()), "conversations-${safe(list)}.json")

    fun writeConversations(rows: List<Wire.Conversation>, list: String) {
        write(ListSerializer(Wire.Conversation.serializer()), rows, "conversations-${safe(list)}.json")
    }

    fun readMessages(threadId: String): List<Wire.Message>? =
        read(ListSerializer(Wire.Message.serializer()), "thread-${safe(threadId)}.json")

    fun writeMessages(messages: List<Wire.Message>, threadId: String) {
        write(ListSerializer(Wire.Message.serializer()), messages, "thread-${safe(threadId)}.json")
    }

    /** Signing out takes the mailbox with it. */
    fun clear() {
        runCatching { dir.deleteRecursively() }
    }

    private fun <T> read(serializer: kotlinx.serialization.KSerializer<T>, name: String): T? {
        val file = File(dir, name)
        if (!file.exists()) return null
        return runCatching { json.decodeFromString(serializer, file.readText()) }
            .getOrElse {
                // A file that no longer parses is yesterday's schema.
                // Deleting it is the repair.
                file.delete()
                null
            }
    }

    private fun <T> write(serializer: kotlinx.serialization.KSerializer<T>, value: T, name: String) {
        runCatching {
            dir.mkdirs()
            File(dir, name).writeText(json.encodeToString(serializer, value))
        }
    }

    /**
     * A thread id is the server's and a list name is ours, but both end
     * up as a file name — so a slash in either would write outside this
     * directory.
     */
    private fun safe(name: String): String = name.map { if (it.isLetterOrDigit()) it else '_' }.joinToString("")
}
