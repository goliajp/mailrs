package jp.golia.mailrs.wire

import kotlinx.serialization.Serializable

/**
 * The address of a mail server, as typed and as kept. The same rule as
 * `ServerAddress` on iOS: the two apps must agree on which typed strings
 * are one server.
 */
object ServerAddress {
    /**
     * `mail.example.com`, `https://mail.example.com/` and
     * `HTTPS://Mail.Example.com` are one server; this is the form it is
     * stored and compared in. Null when what was typed cannot be one.
     */
    fun normalise(text: String): String? {
        val trimmed = text.trim()
        if (trimmed.isEmpty() || trimmed.any { it.isWhitespace() }) return null

        var scheme = "https"
        var rest = trimmed
        val mark = trimmed.indexOf("://")
        if (mark >= 0) {
            scheme = trimmed.substring(0, mark).lowercase()
            rest = trimmed.substring(mark + 3)
        }
        if (scheme != "http" && scheme != "https") return null
        // a query, a fragment or a user name is not part of where a server is
        if (rest.any { it in "?#@" }) return null

        val slash = rest.indexOf('/').takeIf { it >= 0 } ?: rest.length
        val path = rest.substring(slash).trimEnd('/')
        val authority = authority(rest.substring(0, slash)) ?: return null
        return scheme + "://" + authority + path
    }

    /** Host and optional port, lowercased; null when either is malformed. */
    private fun authority(text: String): String? {
        val host: String
        var port = ""
        if (text.startsWith("[")) {
            // an IPv6 literal carries colons of its own
            val close = text.indexOf(']')
            if (close < 0) return null
            host = text.substring(0, close + 1)
            val after = text.substring(close + 1)
            if (after.isNotEmpty() && !after.startsWith(":")) return null
            port = after.drop(1)
            val inner = host.substring(1, host.length - 1)
            if (inner.isEmpty() || !inner.all { it.isHexDigit() || it == ':' || it == '.' }) return null
        } else {
            val pieces = text.split(":")
            if (pieces.size > 2) return null
            host = pieces[0]
            if (pieces.size == 2) {
                port = pieces[1]
                if (port.isEmpty()) return null
            }
            if (!looksLikeHost(host)) return null
        }
        if (port.isEmpty()) return host.lowercase()
        if (!port.all { it in '0'..'9' } || port.length > 5) return null
        val number = port.toInt()
        if (number !in 1..65535) return null
        return host.lowercase() + ":" + number
    }

    private fun Char.isHexDigit() = this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'

    private fun looksLikeHost(host: String): Boolean {
        if (host.isEmpty() || host.contains("..")) return false
        if (host.first() in ".-" || host.last() in ".-") return false
        return host.all { it.isLetterOrDigit() || it in "-._" }
    }

    /**
     * What a row shows for a stored address: the host alone when the
     * connection is encrypted, the whole thing when it is not, so plain
     * `http://` is never hidden behind a tidy name.
     */
    fun display(normalised: String): String = normalised.removePrefix("https://")

    /** The host of a stored address, the name a server gets when none was typed. */
    fun host(normalised: String): String {
        val authority = normalised.substringAfter("://").substringBefore("/")
        if (authority.startsWith("[")) return authority.substringBefore("]") + "]"
        return authority.substringBefore(":")
    }
}

/** One server the reader can sign in to: a name they chose and where it is. */
@Serializable
data class SavedServer(
    val id: String,
    val name: String,
    /** Always in [ServerAddress.normalise] form. */
    val address: String,
)

/**
 * The servers offered on the sign-in screen, and which one worked last.
 *
 * A value with no storage of its own, so every rule here is tested on
 * the JVM; [ServerStore] is the part that reads and writes it. The same
 * rules as `ServerList` on iOS.
 */
@Serializable
data class ServerList(
    val added: List<SavedServer> = emptyList(),
    val builtInHidden: Boolean = false,
    /** The entry of the last successful sign-in, by id. */
    val lastSuccess: String? = null,
) {
    enum class Problem {
        /** What was typed is not a server address. */
        Address,

        /** Another entry already points there. */
        Duplicate,
    }

    /** A change that was refused, and why. */
    class Refused(val problem: Problem) : Exception(problem.name)

    /** What the list shows, built-in first. */
    val visible: List<SavedServer>
        get() {
            if (builtInHidden && added.isNotEmpty()) return added
            return listOf(BUILT_IN) + added
        }

    /** The built-in entry is hidden and can be brought back. */
    val canRestoreBuiltIn: Boolean get() = builtInHidden && added.isNotEmpty()

    /** The built-in entry can be hidden: something else would remain. */
    val canHideBuiltIn: Boolean get() = !builtInHidden && added.isNotEmpty()

    /** Where the sign-in screen starts: the server that last signed in, or the first one listed. */
    val preselected: SavedServer
        get() = visible.firstOrNull { it.id == lastSuccess } ?: visible.first()

    fun entryAt(address: String): SavedServer? {
        val wanted = ServerAddress.normalise(address) ?: return null
        return visible.firstOrNull { it.address == wanted }
    }

    /** Add a server; an empty name becomes the host. Throws [Refused]. */
    fun add(name: String, address: String, id: String = java.util.UUID.randomUUID().toString()): ServerList {
        // typing the hidden built-in address back in is asking for it back
        if (builtInHidden && ServerAddress.normalise(address) == BUILT_IN.address) {
            return copy(builtInHidden = false)
        }
        val checked = check(address, except = null)
        return copy(added = added + SavedServer(id, nameFor(name, checked), checked))
    }

    /** Change a server the reader added; the built-in one is not theirs to change. Throws [Refused]. */
    fun edit(id: String, name: String, address: String): ServerList {
        if (added.none { it.id == id }) return this
        val checked = check(address, except = id)
        return copy(
            added = added.map { entry ->
                when (entry.id) {
                    id -> entry.copy(name = nameFor(name, checked), address = checked)
                    else -> entry
                }
            },
        )
    }

    /** Delete an added server, or hide the built-in one. */
    fun remove(id: String): ServerList {
        val remembered = lastSuccess.takeIf { it != id }
        if (id == BUILT_IN.id) {
            if (!canHideBuiltIn) return this
            return copy(builtInHidden = true, lastSuccess = remembered)
        }
        val left = added.filter { it.id != id }
        // nothing left to show instead of it
        return copy(added = left, builtInHidden = builtInHidden && left.isNotEmpty(), lastSuccess = remembered)
    }

    fun restoreBuiltIn(): ServerList = copy(builtInHidden = false)

    /**
     * A sign-in to [address] succeeded: remember it for next time, and
     * keep it if it was not in the list.
     */
    fun recordSuccess(address: String): ServerList {
        val wanted = ServerAddress.normalise(address) ?: return this
        if (wanted == BUILT_IN.address) return copy(builtInHidden = false, lastSuccess = BUILT_IN.id)
        added.firstOrNull { it.address == wanted }?.let { return copy(lastSuccess = it.id) }
        val entry = SavedServer(java.util.UUID.randomUUID().toString(), ServerAddress.host(wanted), wanted)
        return copy(added = added + entry, lastSuccess = entry.id)
    }

    private fun check(address: String, except: String?): String {
        val checked = ServerAddress.normalise(address) ?: throw Refused(Problem.Address)
        val others = (listOf(BUILT_IN) + added).filter { it.id != except }
        if (others.any { it.address == checked }) throw Refused(Problem.Duplicate)
        return checked
    }

    private fun nameFor(typed: String, address: String): String =
        typed.trim().ifEmpty { ServerAddress.host(address) }

    companion object {
        /**
         * The list a launch starts with when none was ever saved. An
         * install that was signed in before the list existed typed its
         * server by hand; [previous] is that address, and it is kept
         * and chosen so nobody has to find it again.
         */
        fun afterUpgrade(previous: String?): ServerList {
            if (previous == null) return ServerList()
            return ServerList().recordSuccess(previous)
        }

        /**
         * Shipped with the app. It cannot be edited or deleted, only
         * hidden, and it comes back by itself when it would otherwise
         * leave the list empty: a sign-in screen with nowhere to sign
         * in to is a dead end.
         */
        val BUILT_IN = SavedServer("builtin.goliamail", "GoliaMail", "https://mail.golia.ai")
    }
}
