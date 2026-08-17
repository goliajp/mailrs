package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.ui.AdminRow
import jp.golia.mailrs.wire.Admin
import jp.golia.mailrs.wire.accountQuota
import jp.golia.mailrs.wire.accountSieve
import jp.golia.mailrs.wire.accountWebhooks
import jp.golia.mailrs.wire.accounts
import jp.golia.mailrs.wire.addAlias
import jp.golia.mailrs.wire.addDomain
import jp.golia.mailrs.wire.addEmailGroupMember
import jp.golia.mailrs.wire.addToSenderList
import jp.golia.mailrs.wire.agentKeys
import jp.golia.mailrs.wire.aliases
import jp.golia.mailrs.wire.apps
import jp.golia.mailrs.wire.auditLog
import jp.golia.mailrs.wire.deleteAgentKey
import jp.golia.mailrs.wire.deleteAlias
import jp.golia.mailrs.wire.deleteDomain
import jp.golia.mailrs.wire.dmarcReports
import jp.golia.mailrs.wire.domains
import jp.golia.mailrs.wire.emailGroupMembers
import jp.golia.mailrs.wire.emailGroups
import jp.golia.mailrs.wire.groupMembers
import jp.golia.mailrs.wire.groupPermissions
import jp.golia.mailrs.wire.groups
import jp.golia.mailrs.wire.queue
import jp.golia.mailrs.wire.removeEmailGroupMember
import jp.golia.mailrs.wire.removeFromSenderList
import jp.golia.mailrs.wire.senderList
import jp.golia.mailrs.wire.suppressions
import jp.golia.mailrs.wire.MailrsClient
import kotlinx.coroutines.launch

/**
 * The operator's half of the view model.
 *
 * Extensions rather than methods, because Kotlin has no partial classes
 * and `MailViewModel` had reached 1,460 lines against this repo's
 * 500-line limit. The state they read and write is the same single
 * `UiState` — nothing here keeps a second copy of anything.
 *
 * Split by *what it is about* rather than by size: everything here is
 * the directory, the queue and the records an operator looks at, and
 * none of it is on the path a person takes to read their mail.
 */
/**
 * Open an operator list and fetch it.
 *
 * Fetched every time rather than cached: these are answers about
 * what the server is configured to do right now, and a stale one
 * read as current is how an operator concludes a change did not
 * take.
 */
fun MailViewModel.openAdmin(section: AdminSection) {
    _state.value = _state.value.copy(adminOpen = section, busy = true, error = null)
    viewModelScope.launch {
        _state.value = when (section) {
            AdminSection.Accounts -> when (val r = client.accounts()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, accounts = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Aliases -> when (val r = client.aliases()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, aliases = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Domains -> when (val r = client.domains()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, domains = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Queue -> when (val r = client.queue()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, queue = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Dmarc -> when (val r = client.dmarcReports()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, dmarc = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Audit -> when (val r = client.auditLog()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, audit = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.AgentKeys -> when (val r = client.agentKeys()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, agentKeys = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Allowed -> when (val r = client.senderList(allowed = true)) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, allowedSenders = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Blocked -> when (val r = client.senderList(allowed = false)) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, blockedSenders = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Suppressed -> when (val r = client.suppressions()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, suppressed = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Groups -> when (val r = client.groups()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, groups = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.EmailGroups -> when (val r = client.emailGroups()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, emailGroups = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
            AdminSection.Apps -> when (val r = client.apps()) {
                is MailrsClient.Outcome.Ok -> _state.value.copy(busy = false, apps = r.value)
                is MailrsClient.Outcome.Err -> _state.value.copy(busy = false, error = r.message)
            }
        }
    }
}

fun MailViewModel.closeAdmin() {
    _state.value = _state.value.copy(adminOpen = null, adminDetail = null, accountDetail = null)
}

/**
 * Open one group and read who is in it.
 *
 * Both kinds answer members under the same `members` key; only a
 * permission group has grants, and asking for them on an email
 * group would be a request that means nothing.
 */
fun MailViewModel.openAdminRow(section: AdminSection, row: jp.golia.mailrs.ui.AdminRow) {
    if (section == AdminSection.Accounts) {
        openAccount(row.key)
        return
    }
    val id = row.key.toLongOrNull() ?: return
    _state.value = _state.value.copy(
        adminDetail = AdminDetail(section, id, row.headline),
    )
    viewModelScope.launch {
        val members = when (section) {
            AdminSection.EmailGroups -> client.emailGroupMembers(id)
            AdminSection.Groups -> client.groupMembers(id)
            else -> return@launch
        }
        val grants = if (section == AdminSection.Groups) {
            client.groupPermissions(id)
        } else {
            MailrsClient.Outcome.Ok(emptyList())
        }
        val current = _state.value.adminDetail ?: return@launch
        if (current.id != id) return@launch
        _state.value = _state.value.copy(
            adminDetail = current.copy(
                members = (members as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
                grants = (grants as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
                loading = false,
            ),
        )
    }
}

fun MailViewModel.closeAdminRow() {
    _state.value = _state.value.copy(adminDetail = null)
}

private fun MailViewModel.openAccount(address: String) {
    _state.value = _state.value.copy(accountDetail = AccountDetail(address))
    viewModelScope.launch {
        val quota = client.accountQuota(address)
        val sieve = client.accountSieve(address)
        val hooks = client.accountWebhooks(address)
        val current = _state.value.accountDetail ?: return@launch
        if (current.address != address) return@launch
        _state.value = _state.value.copy(
            accountDetail = current.copy(
                quotaBytes = (quota as? MailrsClient.Outcome.Ok)?.value,
                sieve = (sieve as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
                webhooks = (hooks as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
                loading = false,
            ),
        )
    }
}

fun MailViewModel.closeAccount() {
    _state.value = _state.value.copy(accountDetail = null)
}

/**
 * Add or remove a member of an email group.
 *
 * Only email groups: a permission group's membership decides what
 * somebody may do, and granting that from a phone list — with no
 * confirmation and no record of why — is not an edit this offers.
 */
fun MailViewModel.addGroupMember(address: String) {
    val detail = _state.value.adminDetail ?: return
    if (detail.section != AdminSection.EmailGroups || address.isBlank()) return
    viewModelScope.launch {
        val r = client.addEmailGroupMember(detail.id, address.trim())
        if (r is MailrsClient.Outcome.Err) {
            _state.value = _state.value.copy(error = r.message)
            return@launch
        }
        reloadDetail(detail)
    }
}

fun MailViewModel.removeGroupMember(address: String) {
    val detail = _state.value.adminDetail ?: return
    if (detail.section != AdminSection.EmailGroups) return
    viewModelScope.launch {
        val r = client.removeEmailGroupMember(detail.id, address)
        if (r is MailrsClient.Outcome.Err) {
            _state.value = _state.value.copy(error = r.message)
            return@launch
        }
        reloadDetail(detail)
    }
}

private suspend fun MailViewModel.reloadDetail(detail: AdminDetail) {
    val members = client.emailGroupMembers(detail.id)
    val current = _state.value.adminDetail ?: return
    if (current.id != detail.id) return
    _state.value = _state.value.copy(
        adminDetail = current.copy(
            members = (members as? MailrsClient.Outcome.Ok)?.value.orEmpty(),
            loading = false,
        ),
    )
}

/**
 * Whether this list can be added to, and what the form asks for.
 *
 * Two fields at most, because an operator adding an alias on a
 * phone is doing it between other things. Anything that needs more
 * — an account, with a password — is not offered here at all rather
 * than offered badly.
 */
fun MailViewModel.addFields(section: AdminSection): List<String> = when (section) {
    AdminSection.Aliases -> listOf("Source address", "Target address")
    AdminSection.Domains -> listOf("Domain name")
    AdminSection.Allowed, AdminSection.Blocked -> listOf("Address")
    else -> emptyList()
}

/**
 * Create a row from what the form holds, then re-read the list.
 *
 * The alias's domain is taken from its own source address rather
 * than asked for separately: they cannot disagree, and a form that
 * lets them is a form that will be filled in wrong.
 */
fun MailViewModel.addAdminRow(section: AdminSection, values: List<String>) {
    viewModelScope.launch {
        val r = when (section) {
            AdminSection.Aliases -> {
                val source = values.getOrElse(0) { "" }.trim()
                val target = values.getOrElse(1) { "" }.trim()
                if (source.isEmpty() || target.isEmpty()) return@launch
                client.addAlias(
                    Admin.AddAliasRequest(
                        sourceAddress = source,
                        targetAddress = target,
                        domain = source.substringAfter('@', ""),
                    ),
                )
            }
            AdminSection.Domains -> {
                val name = values.getOrElse(0) { "" }.trim()
                if (name.isEmpty()) return@launch
                client.addDomain(name)
            }
            AdminSection.Allowed, AdminSection.Blocked -> {
                val address = values.getOrElse(0) { "" }.trim()
                if (address.isEmpty()) return@launch
                client.addToSenderList(section == AdminSection.Allowed, address)
            }
            else -> return@launch
        }
        if (r is MailrsClient.Outcome.Err) {
            _state.value = _state.value.copy(error = r.message)
            return@launch
        }
        openAdmin(section)
    }
}

/**
 * Remove one row, and re-read the list.
 *
 * Re-read rather than removed locally: the server decides whether a
 * delete took, and a row that disappeared from the screen while the
 * request failed is the operator believing a thing is gone.
 */
fun MailViewModel.deleteAdminRow(section: AdminSection, row: jp.golia.mailrs.ui.AdminRow) {
    viewModelScope.launch {
        val r = when (section) {
            AdminSection.Aliases -> client.deleteAlias(row.key.toLongOrNull() ?: return@launch)
            AdminSection.Domains -> client.deleteDomain(row.key)
            AdminSection.AgentKeys ->
                client.deleteAgentKey(row.key.toLongOrNull() ?: return@launch)
            AdminSection.Allowed -> client.removeFromSenderList(allowed = true, address = row.key)
            AdminSection.Blocked -> client.removeFromSenderList(allowed = false, address = row.key)
            AdminSection.Accounts, AdminSection.Queue, AdminSection.Dmarc,
            AdminSection.Audit, AdminSection.Suppressed,
            AdminSection.Groups, AdminSection.EmailGroups,
            AdminSection.Apps -> return@launch
        }
        if (r is MailrsClient.Outcome.Err) {
            _state.value = _state.value.copy(error = r.message)
            return@launch
        }
        openAdmin(section)
    }
}
