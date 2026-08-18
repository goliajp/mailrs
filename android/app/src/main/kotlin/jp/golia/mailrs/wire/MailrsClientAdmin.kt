package jp.golia.mailrs.wire

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.Request

/**
 * The operator's endpoints, as extensions.
 *
 * Split out when `MailrsClient` reached 546 lines against this repo's
 * 500-line limit, and split here rather than anywhere: everything in
 * this file is `/api/admin` — the directory, the queue, the records —
 * and none of it is on the path a person takes to read their mail.
 *
 * Extensions can reach the `internal` plumbing (`get`, `post`, `one`,
 * `map`), which is the whole reason this works without making any of
 * it public.
 */
// ── Operator ────────────────────────────────────────────────────

suspend fun MailrsClient.accounts(): MailrsClient.Outcome<List<Admin.Account>> =
    one(get("/api/admin/accounts"), Admin.AccountList.serializer()).map { it.items }

suspend fun MailrsClient.aliases(): MailrsClient.Outcome<List<Admin.Alias>> =
    one(get("/api/admin/aliases"), Admin.AliasList.serializer()).map { it.items }

suspend fun MailrsClient.domains(): MailrsClient.Outcome<List<Admin.Domain>> =
    one(get("/api/admin/domains"), Admin.DomainList.serializer()).map { it.items }

suspend fun MailrsClient.queue(): MailrsClient.Outcome<List<Admin.QueueJob>> =
    one(get("/api/admin/queues"), Admin.QueueList.serializer()).map { it.items }

suspend fun MailrsClient.dmarcReports(): MailrsClient.Outcome<List<Admin.DmarcReport>> =
    one(get("/api/admin/dmarc/reports"), Admin.DmarcList.serializer()).map { it.items }

suspend fun MailrsClient.auditLog(): MailrsClient.Outcome<List<Admin.AuditEntry>> =
    one(get("/api/admin/audit-log"), Admin.AuditList.serializer()).map { it.items }

/**
 * `GET /api/mail/messages/{uid}/raw` — the message as it arrived.
 *
 * Answered as `message/rfc822`, not JSON. A client that decoded it
 * would fail on a message that came back perfectly well.
 */
suspend fun MailrsClient.messageSource(uid: Int): MailrsClient.Outcome<String> = get("/api/mail/messages/$uid/raw")

suspend fun MailrsClient.groups(): MailrsClient.Outcome<List<Admin.Group>> =
    one(get("/api/admin/groups"), Admin.GroupList.serializer()).map { it.items }

suspend fun MailrsClient.emailGroups(): MailrsClient.Outcome<List<Admin.EmailGroup>> =
    one(get("/api/admin/email-groups"), Admin.EmailGroupList.serializer()).map { it.items }

suspend fun MailrsClient.emailGroupMembers(id: Long): MailrsClient.Outcome<List<String>> =
    one(get("/api/admin/email-groups/$id/members"), Admin.MemberList.serializer()).map { it.members }

suspend fun MailrsClient.addEmailGroupMember(id: Long, address: String): MailrsClient.Outcome<String> = post(
    url("/api/admin/email-groups/$id/members"),
    json.encodeToString(Admin.AddMemberRequest.serializer(), Admin.AddMemberRequest(address)),
    authorized = true,
)

suspend fun MailrsClient.removeEmailGroupMember(id: Long, address: String): MailrsClient.Outcome<String> =
    delete("/api/admin/email-groups/$id/members/" + enc(address))

suspend fun MailrsClient.groupMembers(id: Long): MailrsClient.Outcome<List<String>> =
    one(get("/api/admin/groups/$id/members"), Admin.MemberList.serializer()).map { it.members }

suspend fun MailrsClient.groupPermissions(id: Long): MailrsClient.Outcome<List<String>> =
    one(get("/api/admin/groups/$id/permissions"), Admin.PermissionList.serializer())
        .map { it.permissions }

suspend fun MailrsClient.apps(): MailrsClient.Outcome<List<Admin.App>> =
    one(get("/api/admin/apps"), Admin.AppList.serializer()).map { it.items }

suspend fun MailrsClient.accountQuota(address: String): MailrsClient.Outcome<Long?> =
    one(get("/api/admin/accounts/" + enc(address) + "/quota"), Admin.Quota.serializer())
        .map { it.quotaBytes }

suspend fun MailrsClient.accountSieve(address: String): MailrsClient.Outcome<String> =
    one(get("/api/admin/accounts/" + enc(address) + "/sieve"), Admin.Sieve.serializer())
        .map { it.script }

suspend fun MailrsClient.accountWebhooks(address: String): MailrsClient.Outcome<List<Admin.Webhook>> =
    one(
        get("/api/admin/accounts/" + enc(address) + "/webhook-subscriptions"),
        Admin.WebhookList.serializer(),
    ).map { it.items }

suspend fun MailrsClient.agentKeys(): MailrsClient.Outcome<List<Admin.AgentKey>> =
    one(get("/api/agent/keys"), Admin.AgentKeyList.serializer()).map { it.items }

suspend fun MailrsClient.deleteAgentKey(id: Long): MailrsClient.Outcome<String> = delete("/api/agent/keys/$id")

suspend fun MailrsClient.suppressions(): MailrsClient.Outcome<List<String>> =
    one(get("/api/admin/suppressions"), Admin.SuppressionList.serializer()).map { it.items }

/** `allowed` is the whitelist, `blocked` the blacklist. */
suspend fun MailrsClient.senderList(allowed: Boolean): MailrsClient.Outcome<List<String>> =
    one(get(senderListPath(allowed)), Admin.SenderList.serializer()).map { it.entries }

suspend fun MailrsClient.addToSenderList(allowed: Boolean, address: String): MailrsClient.Outcome<String> = post(
    url(senderListPath(allowed)),
    json.encodeToString(Admin.AddSenderRequest.serializer(), Admin.AddSenderRequest(address)),
    authorized = true,
)

suspend fun MailrsClient.removeFromSenderList(allowed: Boolean, address: String): MailrsClient.Outcome<String> =
    delete(senderListPath(allowed) + "/" + enc(address))

private fun MailrsClient.senderListPath(allowed: Boolean) =
    if (allowed) "/api/spam/whitelist" else "/api/spam/blacklist"

suspend fun MailrsClient.addAlias(req: Admin.AddAliasRequest): MailrsClient.Outcome<String> = post(
    url("/api/admin/aliases"),
    json.encodeToString(Admin.AddAliasRequest.serializer(), req),
    authorized = true,
)

suspend fun MailrsClient.deleteAlias(id: Long): MailrsClient.Outcome<String> = delete("/api/admin/aliases/$id")

suspend fun MailrsClient.addDomain(name: String): MailrsClient.Outcome<String> = post(
    url("/api/admin/domains"),
    json.encodeToString(Admin.AddDomainRequest.serializer(), Admin.AddDomainRequest(name)),
    authorized = true,
)

suspend fun MailrsClient.deleteDomain(name: String): MailrsClient.Outcome<String> = delete("/api/admin/domains/" + enc(name))

private suspend fun MailrsClient.delete(path: String): MailrsClient.Outcome<String> = withContext(Dispatchers.IO) {
    val s = session ?: return@withContext MailrsClient.Outcome.Err("Not signed in.")
    send(
        Request.Builder()
            .url(s.server + path)
            .header("Authorization", "Bearer ${s.token}")
            .delete()
            .build(),
    )
}

/**
 * `GET /api/mail/sends/{id}/source` — the bytes that actually left.
 *
 * The counterpart to a received message's raw view, and the one worth
 * reaching for when a send failed: what the queue holds is what resend
 * would re-enqueue, headers and all.
 */
suspend fun MailrsClient.sendSource(sendId: String): MailrsClient.Outcome<String> =
    get("/api/mail/sends/${enc(sendId)}/source")

/** `GET /api/admin/dmarc/sources` — a rollup per sending IP. */
suspend fun MailrsClient.dmarcSources(): MailrsClient.Outcome<Wire.DmarcSourceList> =
    one(get("/api/admin/dmarc/sources"), Wire.DmarcSourceList.serializer())
