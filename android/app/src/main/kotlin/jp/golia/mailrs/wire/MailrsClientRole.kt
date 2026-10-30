package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * What the signed-in account may do, as `GET /api/auth/me` reports it.
 *
 * Backend: `crates/webapi/src/handlers/auth/mod.rs` — `AuthMeResponse`.
 * It decides which pages the app offers; the server still refuses what the
 * account may not call, whatever the app shows.
 */
@Serializable
data class Role(
    @SerialName("is_admin") val isAdmin: Boolean = false,
    @SerialName("is_super") val isSuper: Boolean = false,
)

suspend fun MailrsClient.role(): MailrsClient.Outcome<Role> =
    one(get("/api/auth/me"), Role.serializer())

/** A 403 that is not a request for the password: the account lacks a right. */
internal fun MailrsClient.forbidden(): MailrsClient.Outcome.Err {
    onForbidden?.invoke()
    return MailrsClient.Outcome.Err("The server answered 403.")
}
