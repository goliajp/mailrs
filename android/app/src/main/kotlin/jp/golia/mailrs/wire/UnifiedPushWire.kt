package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** Backend: `webapi/src/handlers/unified_push.rs`. */
object UnifiedPushWire {

    @Serializable
    data class State(
        val enabled: Boolean = false,
        @SerialName("vapid_public_key") val vapidPublicKey: String? = null,
        val devices: List<Device> = emptyList(),
    )

    /** Listed by the distributor's host: the endpoint itself is never sent back. */
    @Serializable
    data class Device(
        val id: String,
        val name: String = "",
        val host: String = "",
        @SerialName("created_at") val createdAt: Long = 0,
    )

    @Serializable
    data class Keys(val p256dh: String, val auth: String)

    /** The shape of `wire-contract/requests/unified-push-register.json`. */
    @Serializable
    data class Register(val endpoint: String, val keys: Keys, val name: String)

    @Serializable
    data class Registered(val id: String)
}

private fun idSegment(id: String) = java.net.URLEncoder.encode(id, Charsets.UTF_8).replace("+", "%20")

suspend fun MailrsClient.unifiedPushState() = one(get("/api/push/unifiedpush"), UnifiedPushWire.State.serializer())

suspend fun MailrsClient.registerUnifiedPush(body: UnifiedPushWire.Register) =
    one(
        call("POST", "/api/push/unifiedpush", json.encodeToString(UnifiedPushWire.Register.serializer(), body)),
        UnifiedPushWire.Registered.serializer(),
    )

suspend fun MailrsClient.unregisterUnifiedPush(id: String) = call("DELETE", "/api/push/unifiedpush/${idSegment(id)}")
