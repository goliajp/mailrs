package jp.golia.mailrs.wire

import okhttp3.Request

/** What a refused write says: the person decides whether to try again. */
const val RESTARTING = "The server is restarting. Try again in a moment."

private const val MAX_WAIT_MS = 30_000L

/**
 * A 503: the server restarting during a deploy. A read is sent once more
 * after the `Retry-After` the server gives. A write is never repeated —
 * sending, moving or deleting twice could do it twice — and says so instead.
 */
internal fun MailrsClient.restarting(
    request: Request,
    retryAfter: String?,
    retried: Boolean,
): MailrsClient.Outcome<String> {
    if (request.method != "GET" || retried) return MailrsClient.Outcome.Err(RESTARTING, "restarting")
    Thread.sleep(retryAfterMs(retryAfter))
    return send(request, retried = true)
}

internal fun retryAfterMs(header: String?): Long {
    val seconds = header?.trim()?.toLongOrNull() ?: return 5_000L
    if (seconds <= 0) return 5_000L
    return minOf(seconds * 1000, MAX_WAIT_MS)
}
