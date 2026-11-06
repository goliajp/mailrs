package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Blocking a conversation's sender from one of its messages: the address
 * goes on the blacklist (and off the whitelist) and the conversation to
 * Junk, in one call — `crates/webapi/src/handlers/block_sender.rs`.
 * Unblocking outside the undo is the plain blacklist delete,
 * `removeFromSenderList(allowed = false, …)`.
 */
object WireBlock {
    @Serializable
    data class BlockRequest(val address: String)

    @Serializable
    data class BlockResponse(
        /** As stored: trimmed and lowercased. */
        val address: String,
        /** It was on the whitelist and was taken off; the undo puts it back. */
        @SerialName("was_allowed") val wasAllowed: Boolean,
    )

    @Serializable
    data class UnblockRequest(val address: String, val allow: Boolean)
}

suspend fun MailrsClient.blockSender(threadId: String, address: String): MailrsClient.Outcome<WireBlock.BlockResponse> = one(
    post(
        url("/api/conversations/${enc(threadId)}/block-sender"),
        json.encodeToString(WireBlock.BlockRequest.serializer(), WireBlock.BlockRequest(address)),
        authorized = true,
    ),
    WireBlock.BlockResponse.serializer(),
)

suspend fun MailrsClient.unblockSender(threadId: String, address: String, allow: Boolean): MailrsClient.Outcome<String> = post(
    url("/api/conversations/${enc(threadId)}/unblock-sender"),
    json.encodeToString(WireBlock.UnblockRequest.serializer(), WireBlock.UnblockRequest(address, allow)),
    authorized = true,
)
