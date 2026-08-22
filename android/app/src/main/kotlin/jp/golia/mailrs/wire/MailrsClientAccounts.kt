package jp.golia.mailrs.wire

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.ListSerializer
import okhttp3.Request

/**
 * Mailboxes somewhere else: list, connect, disconnect, and what to
 * fill in for an address.
 *
 * Extensions, like the invite endpoints beside them, so they reach the
 * `internal` plumbing without any of it becoming public.
 */

/** `GET /api/accounts/external` — the mailboxes this person connected. */
suspend fun MailrsClient.externalAccounts(): MailrsClient.Outcome<List<ExternalAccount>> =
    one(get("/api/accounts/external"), ListSerializer(ExternalAccount.serializer()))

/**
 * `GET /api/accounts/external/settings` — what to fill in for an
 * address, before anything is saved.
 *
 * Asked as the address is typed so the secret field can be labelled
 * with the provider's own word for what it wants. Typing a login
 * password into a field labelled 授权码 is a mistake somebody recovers
 * from; typing it into one labelled "Password" and being told
 * `LOGIN failed` is not.
 */
suspend fun MailrsClient.accountSettings(email: String): MailrsClient.Outcome<AccountSettings> =
    one(get("/api/accounts/external/settings?email=" + enc(email)), AccountSettings.serializer())

/**
 * `POST /api/accounts/external` — connect one.
 *
 * An address and a secret. Everything else the server fills in from
 * its provider table, or discovers from DNS.
 */
suspend fun MailrsClient.connectAccount(
    email: String,
    secret: String,
    name: String,
): MailrsClient.Outcome<String> = post(
    url("/api/accounts/external"),
    json.encodeToString(
        ConnectAccountRequest.serializer(),
        ConnectAccountRequest(
            email = email,
            secret = secret,
            displayName = name.ifEmpty { null },
        ),
    ),
    authorized = true,
)

/** `DELETE /api/accounts/external/{id}` — disconnect one. */
suspend fun MailrsClient.disconnectAccount(id: String): MailrsClient.Outcome<String> =
    withContext(Dispatchers.IO) {
        val s = session ?: return@withContext MailrsClient.Outcome.Err("Not signed in.")
        send(
            Request.Builder()
                .url(s.server + "/api/accounts/external/" + enc(id))
                .header("Authorization", "Bearer ${s.token}")
                .delete()
                .build(),
        )
    }
