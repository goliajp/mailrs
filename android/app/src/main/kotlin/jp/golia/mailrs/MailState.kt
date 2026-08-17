package jp.golia.mailrs

import jp.golia.mailrs.ui.AdminRow
import jp.golia.mailrs.wire.Admin
import jp.golia.mailrs.wire.MailList
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.Prefs
import jp.golia.mailrs.wire.Wire

/**
 * Everything the screens read, and the small types that make it up.
 *
 * Split out of `MailViewModel` when that file reached 1,460 lines
 * against this repo's 500-line limit — Kotlin has no partial classes, so
 * the way to shorten a view model is to stop nesting its types inside
 * it. Nothing changed but where they live.
 *
 * `UiState` is one value and the only one: `.claude/rules/frontend/
 * no-rq-mirror.md` is about a screen keeping a second copy of what a
 * store already holds, and the answer here is the same — there is one
 * copy of every fact and the screens read it.
 */
data class UiState(
    val signedIn: Boolean = false,
    val server: String = "",
    val busy: Boolean = false,
    val error: String? = null,
    val conversations: List<Wire.Conversation> = emptyList(),
    /** The thread being read, or null for the list. */
    val open: Wire.Conversation? = null,
    val messages: List<Wire.Message> = emptyList(),
    /** The message being written, or null. */
    val composing: Draft? = null,
    val sending: Boolean = false,
    /** True for one frame after a send lands, so the list can say so. */
    val sent: Boolean = false,
    /** Whose mailbox this is — reply-all must not address it back. */
    val myAddress: String = "",
    /** What the snackbar is offering to undo, or null. */
    val undo: PendingTriage? = null,
    /** What was typed into the search field. */
    val searchTerm: String = "",
    /**
     * Hits, in the server's ranking. Null means no search is on —
     * distinct from an empty list, which means this term matched
     * nothing, and the two say different things to the reader.
     */
    val results: List<Wire.Conversation>? = null,
    val searching: Boolean = false,
    /** Set by the launcher shortcut; the list opens its search and clears it. */
    val openSearch: Boolean = false,
    /** Which list is showing. Its axes scope both the list and the search. */
    val list: MailList = MailList.Inbox,
    /** A page is on its way; the list shows a spinner at its foot. */
    val loadingMore: Boolean = false,
    /** The last page carried nothing new, so there is no more to ask for. */
    val endOfList: Boolean = false,
    /** Saved drafts, newest first, and whether their list is showing. */
    val drafts: List<Wire.Draft> = emptyList(),
    val draftsOpen: Boolean = false,
    /** A draft was just saved; the list screen says so once. */
    val draftSaved: Boolean = false,
    /** The operator list showing, if any, and what it holds. */
    val adminOpen: AdminSection? = null,
    val accounts: List<Admin.Account> = emptyList(),
    val aliases: List<Admin.Alias> = emptyList(),
    val domains: List<Admin.Domain> = emptyList(),
    val queue: List<Admin.QueueJob> = emptyList(),
    val dmarc: List<Admin.DmarcReport> = emptyList(),
    val audit: List<Admin.AuditEntry> = emptyList(),
    val agentKeys: List<Admin.AgentKey> = emptyList(),
    val allowedSenders: List<String> = emptyList(),
    val blockedSenders: List<String> = emptyList(),
    val suppressed: List<String> = emptyList(),
    val groups: List<Admin.Group> = emptyList(),
    val emailGroups: List<Admin.EmailGroup> = emptyList(),
    val apps: List<Admin.App> = emptyList(),
    /** The account whose side state is showing, if one is. */
    val accountDetail: AccountDetail? = null,
    /** The group whose members are showing, if one is. */
    val adminDetail: AdminDetail? = null,
    /** The raw message being read, if any. Null while it is on its way. */
    val sourceOpen: Boolean = false,
    val source: String? = null,
    /** Whether the settings screen is showing. */
    val settingsOpen: Boolean = false,
    /** Light, dark, or the phone's own answer. */
    val appearance: Prefs.Appearance = Prefs.Appearance.System,
    /** Whether the periodic new-mail check runs. */
    val notifyNewMail: Boolean = true,
    /**
     * The account's signature, appended on the way out.
     *
     * Empty until it has been fetched, and empty is a real answer —
     * an account with no signature signs nothing.
     */
    val signature: String = "",
    /** Threads picked out for a bulk action. Empty means not selecting. */
    val selected: Set<String> = emptySet(),
    /** Contact suggestions for the field named by [suggestingFor]. */
    val suggestions: List<String> = emptyList(),
    val suggestingFor: RecipientField? = null,
    /** Which attachment index is being fetched, if any. */
    val openingAttachment: Int? = null,
    /** A file ready to hand to another app. */
    val openFile: OpenedFile? = null,
    /** Where each message's unsubscribe has got to, by uid. */
    val unsubscribing: Map<Int, Unsubscribing> = emptyMap(),
)

/**
 * An operator list, and how to read it.
 *
 * The rows come out of state rather than being fetched by the
 * screen, so the one place that knows what a row says is the same
 * place that knows what deleting one names.
 */
enum class AdminSection(val title: String, val emptyMessage: String) {
    Accounts("Accounts", "No accounts on this server."),
    Aliases("Aliases", "Nothing forwards anywhere."),
    Domains("Domains", "This server answers for no domain."),
    Queue("Queue", "Nothing waiting to go out."),
    Dmarc("DMARC", "No reports yet."),
    Audit("Audit log", "Nothing has happened."),
    AgentKeys("Agent keys", "No keys act as this account."),
    Allowed("Always allowed", "Nothing skips the filter."),
    Blocked("Always blocked", "Nothing is refused on sight."),
    Suppressed("Suppressed", "The sender is retrying everybody."),
    Groups("Permission groups", "No groups are defined."),
    EmailGroups("Email groups", "No distribution addresses."),
    Apps("Apps", "Nothing holds credentials here.");

    fun rows(state: UiState): List<jp.golia.mailrs.ui.AdminRow> = when (this) {
        // Not deletable here: removing an account takes its mail
        // with it, and a delete button beside a list is not where
        // that decision belongs.
        Accounts -> state.accounts.map {
            jp.golia.mailrs.ui.AdminRow(
                key = it.address,
                headline = it.address,
                detail = listOfNotNull(
                    it.displayName.takeIf(String::isNotBlank),
                    if (it.active) null else "inactive",
                    // A quota of zero is *no cap*, not a full
                    // mailbox, so it is left unsaid rather than
                    // printed as "0 B".
                    it.quotaBytes.takeIf { q -> q > 0 }
                        ?.let { q -> jp.golia.mailrs.ui.humanSize(q) },
                ).joinToString(" · "),
                deletable = false,
                drillable = true,
            )
        }
        Aliases -> state.aliases.map {
            jp.golia.mailrs.ui.AdminRow(
                key = it.id.toString(),
                headline = it.sourceAddress + " → " + it.targetAddress,
                detail = it.aliasType,
                deletable = true,
            )
        }
        Domains -> state.domains.map {
            jp.golia.mailrs.ui.AdminRow(
                key = it.name,
                headline = it.name,
                detail = "",
                deletable = true,
            )
        }
        Queue -> state.queue.map { job ->
            // Asked for later is not stuck, and saying so is the
            // whole reason the row reads its own timestamps: a queue
            // where every row looks stuck is a queue nobody reads.
            val scheduled = job.scheduledAt
            val detail = when {
                scheduled != null && scheduled > System.currentTimeMillis() / 1000 ->
                    "scheduled for " + jp.golia.mailrs.ui.RowDate.format(scheduled)
                job.lastError != null ->
                    "attempt ${job.attempts ?: 0} — ${job.lastError}"
                else -> job.status
            }
            jp.golia.mailrs.ui.AdminRow(
                key = job.id.toString(),
                headline = job.recipient.ifBlank { job.sender },
                detail = detail,
                deletable = false,
            )
        }
        Dmarc -> state.dmarc.map { r ->
            jp.golia.mailrs.ui.AdminRow(
                key = r.sid,
                headline = r.orgName.ifBlank { r.sid },
                // Passing against total, because that is what a
                // report is for. A count of rows says nothing about
                // whether anybody's mail was refused.
                detail = "${r.passing}/${r.total} passing · p=${r.p}",
                deletable = false,
            )
        }
        AgentKeys -> state.agentKeys.map { k ->
            jp.golia.mailrs.ui.AdminRow(
                key = k.id.toString(),
                headline = k.name.ifBlank { k.prefix },
                // The prefix and the scopes, because those are what
                // tell two keys apart when one has to be revoked.
                detail = (listOf(k.prefix) + k.scopes).joinToString(" · "),
                deletable = true,
            )
        }
        Allowed -> state.allowedSenders.map {
            jp.golia.mailrs.ui.AdminRow(key = it, headline = it, detail = "", deletable = true)
        }
        Blocked -> state.blockedSenders.map {
            jp.golia.mailrs.ui.AdminRow(key = it, headline = it, detail = "", deletable = true)
        }
        // Not deletable one at a time: the endpoint clears the set,
        // and a delete button that quietly emptied the list would be
        // a different action wearing the same icon.
        Suppressed -> state.suppressed.map {
            jp.golia.mailrs.ui.AdminRow(key = it, headline = it, detail = "", deletable = false)
        }
        Groups -> state.groups.map { g ->
            jp.golia.mailrs.ui.AdminRow(
                key = g.id.toString(),
                headline = g.name,
                // A builtin is cross-domain and cannot be edited
                // away, so saying which is which is the first thing
                // an operator needs from this list.
                detail = listOfNotNull(
                    if (g.isBuiltin) "built in" else g.domain,
                    g.description.takeIf(String::isNotBlank),
                ).joinToString(" · "),
                deletable = false,
                drillable = true,
            )
        }
        Apps -> state.apps.map { a ->
            jp.golia.mailrs.ui.AdminRow(
                key = a.id.toString(),
                headline = a.name.ifBlank { a.appId },
                detail = listOfNotNull(
                    a.ownerAddress.takeIf(String::isNotBlank),
                    if (a.active) null else "inactive",
                    a.scopes.joinToString(", ").takeIf(String::isNotBlank),
                ).joinToString(" · "),
                deletable = false,
            )
        }
        EmailGroups -> state.emailGroups.map { g ->
            jp.golia.mailrs.ui.AdminRow(
                key = g.id.toString(),
                // The address, not the name: mail is sent to the
                // address, and a list keyed on "Support" does not
                // tell an operator what to type.
                headline = g.address.ifBlank { g.name },
                detail = listOfNotNull(
                    g.name.takeIf { it.isNotBlank() && it != g.address },
                    g.description.takeIf(String::isNotBlank),
                ).joinToString(" · "),
                deletable = false,
                drillable = true,
            )
        }
        Audit -> state.audit.map { e ->
            jp.golia.mailrs.ui.AdminRow(
                key = e.id.toString(),
                headline = e.action + " " + e.target,
                detail = listOf(e.actor, jp.golia.mailrs.ui.RowDate.format(e.timestamp))
                    .filter(String::isNotBlank)
                    .joinToString(" · "),
                deletable = false,
            )
        }
    }
}

/** Which recipient line a suggestion belongs to. */
enum class RecipientField { To, Cc, Bcc }

/** A file picked for the message being written. */
data class Attached(val uri: android.net.Uri, val filename: String, val size: Long)

/** How far a one-click unsubscribe has got. */
enum class Unsubscribing { Working, Done, Failed }

/**
 * One account, opened.
 *
 * The three things about an account that are kept somewhere other
 * than the account row: how much it may hold, the sieve script that
 * files its mail, and what is subscribed to its events. All three
 * are read-only here — a sieve script is a program, and a phone
 * keyboard is the wrong place to edit one.
 */
data class AccountDetail(
    val address: String,
    val quotaBytes: Long? = null,
    val sieve: String = "",
    val webhooks: List<Admin.Webhook> = emptyList(),
    val loading: Boolean = true,
)

/**
 * One group, opened.
 *
 * A group is a list with a list inside it, and the inner one is the
 * point: "Support" says nothing, "Support — lihao@golia.jp" is the
 * answer somebody came for. `grants` is only populated for a
 * permission group, where what it allows matters as much as who is
 * in it.
 */
data class AdminDetail(
    val section: AdminSection,
    val id: Long,
    val title: String,
    val members: List<String> = emptyList(),
    val grants: List<String> = emptyList(),
    val loading: Boolean = true,
)

/** A downloaded attachment, waiting for the screen to hand it on. */
data class OpenedFile(val file: java.io.File, val mimeType: String, val filename: String)

/**
 * A triage waiting out its undo window.
 *
 * `before` is the whole list as it was, not just the row: putting
 * one row back at the right index is the same information and more
 * ways to get it wrong.
 */
data class PendingTriage(
    val conversation: Wire.Conversation,
    val verb: MailrsClient.Verb,
    val before: List<Wire.Conversation>,
)

/**
 * A message being written.
 *
 * `id` exists so the composer's fields reset when a *different*
 * draft opens and not on every recomposition — `remember(draft.id)`
 * rather than `remember(Unit)`, which would keep the previous
 * reply's text when you opened a second one.
 */
/**
 * The message being written, held here and nowhere else.
 *
 * The composer used to mirror these into its own `remember`d state
 * and hand them back on send. That is the pattern this codebase has
 * a rule against (`frontend/no-rq-mirror.md`), and it had a second
 * cost here: the back gesture cancels through the shell, which
 * cannot see a screen's local variables, so leaving by the gesture
 * everybody uses would have thrown the text away.
 *
 * `serverId` is null until the draft has been saved once; after
 * that it is reused, or one message leaves a trail of drafts.
 */
@kotlinx.serialization.Serializable
data class Draft(
    val id: Int,
    val to: String = "",
    val cc: String = "",
    val bcc: String = "",
    val subject: String = "",
    val body: String = "",
    val inReplyTo: String? = null,
    val replyToThreadId: String? = null,
    val serverId: Long? = null,
    /**
     * The message being forwarded, by uid.
     *
     * Set only for a forward. The server re-extracts that message's
     * attachments and sends them along, so what is passed on is what
     * arrived rather than a copy this phone had to fetch first.
     */
    val forwardFrom: Int? = null,
    /**
     * Files picked to go with it.
     *
     * In memory only, and deliberately: a server draft has nowhere
     * to keep an attachment, and a `content://` URI granted to this
     * activity does not survive the process — a draft reopened
     * tomorrow with a file it can no longer read would be worse than
     * one that says it has none.
     */
    @kotlinx.serialization.Transient
    val attachments: List<Attached> = emptyList(),
) {
    /** Nothing typed and nothing quoted: not worth saving. */
    val isEmpty: Boolean
        get() = to.isBlank() && cc.isBlank() && bcc.isBlank() &&
            subject.isBlank() && body.isBlank()
}
