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
}
