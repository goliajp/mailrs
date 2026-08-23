package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/*
 * The wire types for mailboxes somewhere else.
 *
 * Split from `Wire.kt` when it passed the 500-line limit, along the
 * line that was already there: these four are one feature and that
 * file is everything else. Top-level rather than inside `Wire`,
 * because `object Wire` cannot span two files — so the call sites drop
 * the prefix, which is the whole cost of the split.
 */

/**
 * A mailbox somewhere else, as the server stores it.
 *
 * **There is no secret on it.** The password goes to the server
 * once, sealed there, and no route returns it — not even to the
 * person who typed it.
 */
@Serializable
data class ExternalAccount(
    val id: String = "",
    val email: String = "",
    @SerialName("display_name") val displayName: String = "",
    val provider: String = "custom",
    /** `#rrggbb`, chosen by the server so all clients agree. */
    val colour: String? = null,
    /** `ok` / `needs_auth` / `error` / `paused`. */
    val state: String = "ok",
    @SerialName("last_error") val lastError: String? = null,
    /** What it is doing right now — a re-read is work, not a fault. */
    @SerialName("progress") val progress: String? = null,
) {
    /**
     * What the row says on screen. The two failures need different
     * words: one is a button to press, the other is waiting.
     */
    val trouble: String?
        get() = when (state) {
            "needs_auth" -> "Sign in again"
            "error" -> "Not syncing"
            "paused" -> "Paused"
            else -> null
        }
}

/** What a set-up screen should fill in for an address. */
@Serializable
data class AccountSettings(
    val known: Boolean = false,
    val preset: Preset? = null,
) {
    @Serializable
    data class Preset(
        val id: String = "",
        val label: String = "",
        /** `password` / `app_password` / `oauth2`. */
        val auth: String = "password",
        @SerialName("secret_help") val secretHelp: SecretHelp? = null,
    )

    /** Where to get what this provider wants, in its own words. */
    @Serializable
    data class SecretHelp(val what: String = "", val url: String = "")
}

/** The body that connects a mailbox: an address and a secret. */
/**
 * What `GET /api/accounts/external` answers.
 *
 * **An object with one key, not a bare array.** Deserialising it as a
 * list fails, and the screen showed an empty list — which reads as
 * "you have not connected anything" rather than as a fault. No test
 * caught it because the shared stub did not serve this route at all.
 */
@Serializable
data class ExternalAccountList(
    val accounts: List<ExternalAccount> = emptyList(),
)

@Serializable
data class ConnectAccountRequest(
    val email: String,
    val secret: String,
    @SerialName("display_name") val displayName: String? = null,
    /** The account's own name on that server, when it is not the address. */
    val username: String? = null,
    /** Where to read from, when nobody could work it out. */
    val incoming: WireEndpoint? = null,
    /** Where to send through. */
    val outgoing: WireEndpoint? = null,
)

/** One server, as the API wants it. */
@Serializable
data class WireEndpoint(
    val host: String,
    val port: Int,
    val protocol: String,
    val tls: String,
)
