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
