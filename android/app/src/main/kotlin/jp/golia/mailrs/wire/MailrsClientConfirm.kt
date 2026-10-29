package jp.golia.mailrs.wire

import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/** The error code of an operation that needs the password again. */
const val CONFIRM_REQUIRED = "confirm_required"

internal fun MailrsClient.errorCode(text: String): String? =
    runCatching { json.parseToJsonElement(text).jsonObject["error"]?.jsonPrimitive?.content }.getOrNull()

/**
 * A 401 from `POST /api/auth/confirm` that leaves the session alone: a
 * wrong password, or a missing one-time code. Read as "signed out", the
 * first mistyped password would sign the reader out. The fifth wrong
 * try does end the session (`session_revoked`), and is left to the
 * ordinary 401 path.
 */
internal fun MailrsClient.confirmRefusal(text: String): MailrsClient.Outcome.Err? = when (errorCode(text)) {
    "confirm_failed" -> {
        val left = runCatching {
            json.parseToJsonElement(text).jsonObject["remaining"]?.jsonPrimitive?.content?.toInt()
        }.getOrNull() ?: 0
        MailrsClient.Outcome.Err(wrongPassword(left), "confirm_failed")
    }
    "totp_required" -> MailrsClient.Outcome.Err("Enter the code from your authenticator app.", "totp_required")
    else -> null
}

private fun wrongPassword(left: Int): String = when (left) {
    1 -> "Wrong password or code. 1 try left before you are signed out."
    else -> "Wrong password or code. $left tries left before you are signed out."
}

/**
 * Confirm this session's password, as the server asks before changes a
 * stolen session could abuse. Lasts ten minutes on the server.
 *
 * Backend: crates/webapi/src/session/confirm.rs — fn confirm.
 */
suspend fun MailrsClient.confirm(password: String, code: String?): MailrsClient.Outcome<String> {
    val body = buildJsonObject {
        put("password", password)
        if (code != null) put("code", code)
    }
    return post(url("/api/auth/confirm"), body.toString(), authorized = true)
}
