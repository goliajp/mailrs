package jp.golia.mailrs.wire

import kotlinx.serialization.Serializable
import kotlinx.serialization.SerialName
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** The error code of a sign-in to an account whose password an administrator cleared. */
const val PASSWORD_CLEARED = "password_cleared"

/** The error code of a reset code that is wrong, used up or replaced. */
const val INVALID_RESET_CODE = "invalid_reset_code"

/** Whom to ask, as a sentence part: "Li Hao (lihao@golia.jp)", or an administrator. */
fun contactText(name: String, address: String): String = when {
    name.isNotEmpty() && address.isNotEmpty() -> "$name ($address)"
    name.isNotEmpty() -> name
    address.isNotEmpty() -> address
    else -> "an administrator"
}

/**
 * Whom to ask, when `text` is the 401 a sign-in to an account whose
 * password was cleared gets; null for any other body.
 * Backend: crates/webapi/src/handlers/auth/session.rs — fn password_cleared_response
 */
fun clearedContact(text: String): String? {
    val body = runCatching { Json.parseToJsonElement(text).jsonObject }.getOrNull() ?: return null
    if (body["error"]?.jsonPrimitive?.content != PASSWORD_CLEARED) return null
    val contact = runCatching { body["contact"]?.jsonObject }.getOrNull()
    val name = contact?.get("name")?.jsonPrimitive?.content.orEmpty()
    val address = contact?.get("address")?.jsonPrimitive?.content.orEmpty()
    return contactText(name, address)
}

/** That 401 as an error carrying whom to ask as its message. */
internal fun MailrsClient.passwordCleared(text: String): MailrsClient.Outcome.Err? =
    clearedContact(text)?.let { MailrsClient.Outcome.Err(it, PASSWORD_CLEARED) }

@Serializable
private data class ResetRequest(
    val address: String,
    val code: String,
    @SerialName("new_password") val newPassword: String,
)

/**
 * `POST /api/auth/password-reset`: a new password, with the code an issuer
 * handed out. Nothing is signed in; the person signs in afterwards.
 */
suspend fun MailrsClient.resetPassword(
    server: String,
    address: String,
    code: String,
    newPassword: String,
): MailrsClient.Outcome<Unit> {
    val base = baseUrlOverride ?: normalise(server)
    val body = json.encodeToString(ResetRequest.serializer(), ResetRequest(address, code, newPassword))
    return when (val r = post("$base/api/auth/password-reset", body, authorized = false)) {
        is MailrsClient.Outcome.Ok -> MailrsClient.Outcome.Ok(Unit)
        is MailrsClient.Outcome.Err if r.code == INVALID_RESET_CODE -> MailrsClient.Outcome.Err(
            "That code does not work: it may be mistyped, used up, or replaced by a newer one.",
            INVALID_RESET_CODE,
        )
        // a refused new password says why in the server's words
        is MailrsClient.Outcome.Err -> MailrsClient.Outcome.Err(r.code ?: r.message, r.code)
    }
}
