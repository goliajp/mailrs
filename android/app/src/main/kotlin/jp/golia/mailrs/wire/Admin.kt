package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The operator's half of the API.
 *
 * Written against the handlers and the wire types they return, per
 * `.claude/rules/frontend/wire-schema-verification.md`:
 * `crates/core-api/src/method/admin/directory.rs` for accounts, aliases
 * and domains; `crates/webapi/src/handlers/admin_directory.rs` for the
 * routes. Every list answers `{items: [...]}` rather than a bare array —
 * the mail endpoints do the opposite, and mixing them up produces a
 * parse failure that reads as an empty list.
 */
object Admin {

    @Serializable
    data class Account(
        val address: String,
        val domain: String = "",
        @SerialName("display_name") val displayName: String = "",
        val active: Boolean = true,
        @SerialName("created_at") val createdAt: Long = 0,
        @SerialName("quota_bytes") val quotaBytes: Long = 0,
        @SerialName("recovery_email") val recoveryEmail: String? = null,
    )

    @Serializable
    data class Alias(
        val id: Long,
        @SerialName("source_address") val sourceAddress: String,
        @SerialName("target_address") val targetAddress: String,
        val domain: String = "",
        @SerialName("alias_type") val aliasType: String = "alias",
        val active: Boolean = true,
        @SerialName("created_at") val createdAt: Long = 0,
    )

    @Serializable
    data class Domain(
        val name: String,
        @SerialName("created_at") val createdAt: Long = 0,
    )

    @Serializable
    data class AccountList(val items: List<Account> = emptyList())

    @Serializable
    data class AliasList(val items: List<Alias> = emptyList())

    @Serializable
    data class DomainList(val items: List<Domain> = emptyList())

    /**
     * One message the sender has not finished with.
     *
     * `scheduled_at` in the future is *asked for later*, not stuck —
     * before the row read its own timestamps the two were
     * indistinguishable, and a queue where every row looks stuck is a
     * queue nobody reads.
     *
     * Backend: `crates/webapi/src/handlers/complete.rs`, `/api/admin/queues`.
     */
    @Serializable
    data class QueueJob(
        val id: Long,
        val sender: String = "",
        val recipient: String = "",
        val status: String = "",
        val attempts: Int? = null,
        @SerialName("last_error") val lastError: String? = null,
        @SerialName("next_retry") val nextRetry: Long? = null,
        @SerialName("scheduled_at") val scheduledAt: Long? = null,
        @SerialName("created_at") val createdAt: Long? = null,
    )

    /**
     * A DMARC report: somebody else saying what your mail looked like
     * from their side, so `passing` against `total` is the whole point
     * of the row.
     *
     * Backend: `crates/webapi/src/handlers/dmarc.rs` — `ReportSummary`.
     */
    @Serializable
    data class DmarcReport(
        val sid: String,
        @SerialName("org_name") val orgName: String = "",
        val email: String = "",
        @SerialName("policy_domain") val policyDomain: String = "",
        val begin: Long = 0,
        val end: Long = 0,
        val p: String = "",
        val total: Long = 0,
        val passing: Long = 0,
        val rows: Long = 0,
    )

    /** Backend: `crates/webapi/src/handlers/admin.rs` — `list_audit_log`. */
    @Serializable
    data class AuditEntry(
        val id: Long,
        val timestamp: Long = 0,
        val actor: String = "",
        val action: String = "",
        val target: String = "",
        val detail: String = "",
    )

    @Serializable
    data class QueueList(val items: List<QueueJob> = emptyList())

    @Serializable
    data class DmarcList(val items: List<DmarcReport> = emptyList())

    @Serializable
    data class AuditList(val items: List<AuditEntry> = emptyList())

    /**
     * An API key that acts as this account.
     *
     * The stored record has no secret in it — the server keeps eight
     * characters — so a client that expected one would decode nothing.
     */
    @Serializable
    data class AgentKey(
        val id: Long,
        val name: String = "",
        val scopes: List<String> = emptyList(),
        val prefix: String = "",
        @SerialName("created_at") val createdAt: Long = 0,
    )

    @Serializable
    data class AgentKeyList(val items: List<AgentKey> = emptyList())

    /** Addresses the sender refuses to try again. */
    @Serializable
    data class SuppressionList(val items: List<String> = emptyList())

    /**
     * A per-user allow or block list.
     *
     * `entries`, not `items` — `spam_lists.rs` answers with a different
     * key from the admin lists, and reaching for the wrong one decodes
     * an empty list, which on screen is indistinguishable from "nothing
     * is listed".
     */
    @Serializable
    data class SenderList(val entries: List<String> = emptyList())

    @Serializable
    data class AddSenderRequest(val address: String)

    /**
     * A permission group. `domain` is absent for the cross-domain
     * builtins, which is why it is nullable rather than empty-by-
     * default: "every domain" and "the empty domain" are different
     * things to say about a group that grants administration.
     *
     * Backend: `crates/core-api/src/method/admin/permissions.rs`.
     */
    @Serializable
    data class Group(
        val id: Long,
        val name: String = "",
        val domain: String? = null,
        val description: String = "",
        @SerialName("is_builtin") val isBuiltin: Boolean = false,
        @SerialName("created_at") val createdAt: Long = 0,
    )

    @Serializable
    data class GroupList(val items: List<Group> = emptyList())

    /**
     * A distribution address: mail to it reaches everyone in it.
     *
     * Backend: `crates/core-api/src/method/admin/directory.rs` —
     * `EmailGroupWire`. Distinct from a **permission** group, which
     * grants what somebody may do; the two share the word "group" and
     * nothing else.
     */
    @Serializable
    data class EmailGroup(
        val id: Long,
        val address: String = "",
        val domain: String = "",
        val name: String = "",
        val description: String = "",
        @SerialName("created_at") val createdAt: Long = 0,
    )

    @Serializable
    data class EmailGroupList(val items: List<EmailGroup> = emptyList())

    /** `{members: [...]}` — both group kinds answer with this key. */
    @Serializable
    data class MemberList(val members: List<String> = emptyList())

    /** `{permissions: [...]}` — what a permission group grants. */
    @Serializable
    data class PermissionList(val permissions: List<String> = emptyList())

    @Serializable
    data class AddMemberRequest(@SerialName("member_address") val memberAddress: String)

    @Serializable
    data class AddAliasRequest(
        @SerialName("source_address") val sourceAddress: String,
        @SerialName("target_address") val targetAddress: String,
        val domain: String,
        @SerialName("alias_type") val aliasType: String = "alias",
    )

    @Serializable
    data class AddDomainRequest(val name: String)
}
