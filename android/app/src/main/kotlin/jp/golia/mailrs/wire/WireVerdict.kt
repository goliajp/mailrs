package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Why a message was judged as it was.
 *
 * Sources: `mailrs_inbound::reason::Reason` (crates/inbound/src/reason.rs),
 * `LinkWarning` (crates/webapi/src/handlers/message_reasons.rs), and the
 * stored verdict `mailrs_inbound::verdict::FraudVerdict` as
 * `get_fraud_verdict` (crates/webapi/src/handlers/quarantine.rs) returns it.
 */
object WireVerdict {
    /** A fraud rule id or a reason code, and the values its sentence names. */
    @Serializable
    data class Reason(
        val code: String,
        // left out by the server when empty
        val params: Map<String, String> = emptyMap(),
    )

    /** A link worth a second look; `shown` only when its text shows another host. */
    @Serializable
    data class LinkWarning(
        val host: String,
        val shown: String? = null,
        val reasons: List<Reason>,
    )

    @Serializable
    data class Layer(
        val name: String,
        /** `pass`, `fail` or `not_applicable`. */
        val outcome: String,
        /** English, for a code this client has no sentence for. */
        val detail: String = "",
        // absent on verdicts stored before reasons existed
        val reasons: List<Reason> = emptyList(),
        val score: Double,
    )

    @Serializable
    data class Verdict(
        /** Empty on a record that only says why unsuspected mail was filed where it was. */
        val layers: List<Layer>,
        /** Why it was filed where it was: Junk, a sender rule, the classifier, a filter. */
        val placement: List<Reason> = emptyList(),
        val quarantined: Boolean,
        @SerialName("rules_version") val rulesVersion: String,
    )

    /**
     * `{"verdict": … | null}`. Null is the ordinary answer: nobody
     * suspected the message and nothing sorted it, which is not the same
     * as "examined and cleared".
     */
    @Serializable
    data class Response(val verdict: Verdict? = null)
}

/** `GET /api/messages/{message_id}/fraud-verdict`. */
suspend fun MailrsClient.fraudVerdict(messageId: String): MailrsClient.Outcome<WireVerdict.Verdict?> =
    one(get("/api/messages/${enc(messageId)}/fraud-verdict"), WireVerdict.Response.serializer())
        .map { it.verdict }
