package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The phone numbers a sender gave the signed-in reader before, in any
 * conversation, as `GET /api/senders/{address}/callback-numbers` answers
 * them (crates/webapi/src/handlers/callback_numbers.rs).
 */
object WireCallbackNumbers {
    /**
     * One number, digits only, and the earliest message that gave it, in
     * the form of a thread message's own `message_id`. The history holds
     * the numbers of the message being read too, first given by it.
     */
    @Serializable
    data class Given(
        val number: String,
        @SerialName("first_seen") val firstSeen: Long,
        @SerialName("first_message_id") val firstMessageId: String,
    )

    @Serializable
    data class Response(val sender: String, val numbers: List<Given>)
}

/** `GET /api/senders/{address}/callback-numbers`; [address] is the bare, lowercased address. */
suspend fun MailrsClient.callbackNumbers(address: String): MailrsClient.Outcome<List<WireCallbackNumbers.Given>> =
    one(get("/api/senders/${enc(address)}/callback-numbers"), WireCallbackNumbers.Response.serializer())
        .map { it.numbers }
