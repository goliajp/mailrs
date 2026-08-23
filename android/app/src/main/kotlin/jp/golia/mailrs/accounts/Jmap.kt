package jp.golia.mailrs.accounts

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive

/**
 * Reading a JMAP mailbox — RFC 8620 / 8621.
 *
 * Ordinary HTTPS rather than a socket, so this needs no session of its
 * own. What it does need is the two shapes a JMAP client can get
 * wrong: finding the API url and the account id in the session object,
 * and knowing when `Email/changes` has given up.
 */
object Jmap {
    private val json = Json { ignoreUnknownKeys = true }

    const val MAIL_CAPABILITY = "urn:ietf:params:jmap:mail"

    /** What `/.well-known/jmap` answers, as far as this client uses it. */
    data class Session(val apiUrl: String, val accountId: String)

    /**
     * Read the session object.
     *
     * `primaryAccounts` is what names the mail account; picking the
     * first key of `accounts` instead works until somebody has two,
     * and then it silently reads the wrong mailbox.
     */
    fun session(body: String): Session? = runCatching {
        val top = json.parseToJsonElement(body).jsonObject
        val apiUrl = top["apiUrl"]?.jsonPrimitive?.contentOrNull.orEmpty()
        if (apiUrl.isEmpty()) return null

        val primary = top["primaryAccounts"] as? JsonObject
        val named = primary?.get(MAIL_CAPABILITY)?.jsonPrimitive?.contentOrNull
        if (!named.isNullOrEmpty()) return Session(apiUrl, named)

        // A server with exactly one account and no primaryAccounts is
        // unambiguous; more than one without it is not, and guessing
        // there would read somebody else's mailbox.
        val accounts = top["accounts"] as? JsonObject
        if (accounts != null && accounts.size == 1) {
            return Session(apiUrl, accounts.keys.first())
        }
        null
    }.getOrNull()

    /** What an `Email/changes` response means for the caller. */
    sealed interface Changes {
        data class Some(val created: List<String>, val newState: String) : Changes

        /**
         * The server cannot answer from this state.
         *
         * **Not an error.** RFC 8620 5.2: the client is told to start
         * over with `Email/query`, and treating it as a failure leaves
         * an account that never syncs again.
         */
        data object StartOver : Changes
    }

    /** Read an `Email/changes` reply. */
    fun changes(body: String): Changes? = runCatching {
        val top = json.parseToJsonElement(body).jsonObject
        val responses = top["methodResponses"] as? JsonArray ?: return null
        val first = responses.firstOrNull()?.jsonArray ?: return null
        if (first.size < 2) return null
        val name = first[0].jsonPrimitive.contentOrNull.orEmpty()
        val payload = first[1] as? JsonObject ?: return null

        if (name == "error") {
            val type = payload["type"]?.jsonPrimitive?.contentOrNull.orEmpty()
            return if (type == "cannotCalculateChanges") Changes.StartOver else null
        }
        val newState = payload["newState"]?.jsonPrimitive?.contentOrNull ?: return null
        val created = (payload["created"] as? JsonArray)
            ?.mapNotNull { it.jsonPrimitive.contentOrNull }
            .orEmpty()
        Changes.Some(created, newState)
    }.getOrNull()

    /**
     * The newest messages, in **one** round trip.
     *
     * The back-reference (`#ids`) is what makes it one: it tells the
     * server to feed the ids from the query straight into the get. A
     * client that does not use it asks, waits, and asks again — which
     * on a phone is two of everything, including the latency.
     */
    fun newestRequest(accountId: String, limit: Int = 50): String = """
        {"using":["urn:ietf:params:jmap:core","$MAIL_CAPABILITY"],
         "methodCalls":[
           ["Email/query",{"accountId":"$accountId",
             "sort":[{"property":"receivedAt","isAscending":false}],
             "limit":$limit},"0"],
           ["Email/get",{"accountId":"$accountId",
             "#ids":{"resultOf":"0","name":"Email/query","path":"/ids"},
             "properties":["id","subject","from","receivedAt","keywords","messageId"]},"1"]
         ]}
    """.trimIndent().replace("\n", "").replace("  ", "")

    /** One message, as far as a list row needs it. */
    data class Email(
        val id: String,
        val subject: String,
        val sender: String,
        /** Seconds since the epoch, or null when `receivedAt` was unreadable. */
        val receivedAt: Long?,
        val seen: Boolean,
        val messageId: String,
    )

    /**
     * Read an `Email/get` reply.
     *
     * Three shapes worth naming, because each is silently wrong if
     * guessed:
     *
     * - `from` is a **list of objects**, not a string. Reading it as
     *   text gives an empty sender on every row.
     * - `keywords` says what is true, so `$seen` **absent** means
     *   unread — the same absence that IMAP's flag list uses.
     * - `receivedAt` is a UTC date string, not a number.
     */
    fun emails(body: String): List<Email>? = runCatching {
        val top = json.parseToJsonElement(body).jsonObject
        val responses = top["methodResponses"] as? JsonArray ?: return null
        // The get is not always second: a server may answer in any
        // order, and one that pushes a `Core/echo` in front shifts it.
        val get = responses.map { it.jsonArray }.firstOrNull {
            it.size >= 2 && it[0].jsonPrimitive.contentOrNull == "Email/get"
        } ?: return null
        val list = (get[1] as? JsonObject)?.get("list") as? JsonArray ?: return null
        list.map { element ->
            val e = element.jsonObject
            val from = (e["from"] as? JsonArray)?.firstOrNull()?.jsonObject
            val name = from?.get("name")?.jsonPrimitive?.contentOrNull.orEmpty()
            val email = from?.get("email")?.jsonPrimitive?.contentOrNull.orEmpty()
            val keywords = e["keywords"] as? JsonObject
            Email(
                id = e["id"]?.jsonPrimitive?.contentOrNull.orEmpty(),
                subject = e["subject"]?.jsonPrimitive?.contentOrNull.orEmpty(),
                sender = when {
                    name.isNotEmpty() && email.isNotEmpty() -> "$name <$email>"
                    else -> name.ifEmpty { email }
                },
                receivedAt = utcDate(e["receivedAt"]?.jsonPrimitive?.contentOrNull),
                seen = keywords?.containsKey("\$seen") == true,
                messageId = (e["messageId"] as? JsonArray)
                    ?.firstOrNull()?.jsonPrimitive?.contentOrNull.orEmpty(),
            )
        }
    }.getOrNull()

    /**
     * `2026-08-24T01:46:40Z`, to seconds.
     *
     * Hand-read rather than handed to a date formatter: JMAP's UTCDate
     * is one fixed shape, and a formatter would bring a locale and a
     * default time zone with it — which is how a message moves by hours
     * for somebody who is not in UTC.
     */
    fun utcDate(text: String?): Long? {
        val t = text?.trim().orEmpty()
        if (t.length < 20 || t[10] != 'T' || !t.endsWith("Z")) return null
        val year = t.substring(0, 4).toIntOrNull() ?: return null
        val month = t.substring(5, 7).toIntOrNull() ?: return null
        val day = t.substring(8, 10).toIntOrNull() ?: return null
        val hour = t.substring(11, 13).toIntOrNull() ?: return null
        val minute = t.substring(14, 16).toIntOrNull() ?: return null
        val second = t.substring(17, 19).toIntOrNull() ?: return null
        if (month !in 1..12 || day !in 1..31 || hour > 23 || minute > 59 || second > 60) {
            return null
        }
        return MailDate.epochFromCivil(year, month, day, hour, minute, second)
    }
}
