package jp.golia.mailrs.wire

/** Where this phone's instant notifications stand, for the settings page. */
sealed interface PushStatus {
    /** The server has no Web Push key, so it sends no pushes at all. */
    data object ServerOff : PushStatus

    /** No distributor app on the phone: only the periodic check. */
    data object NoDistributor : PushStatus

    /** A distributor is installed and not in use. */
    data object Off : PushStatus

    /** Asked; the distributor or the server has not answered yet. */
    data class Waiting(val pkg: String) : PushStatus

    data class On(val pkg: String) : PushStatus

    data class Failed(val pkg: String, val reason: String) : PushStatus

    companion object {
        fun of(
            serverEnabled: Boolean,
            installed: List<String>,
            chosen: String?,
            have: PushSync.Snapshot,
            failure: String?,
        ): PushStatus {
            if (!serverEnabled) return ServerOff
            if (installed.isEmpty()) return NoDistributor
            if (chosen == null) return Off
            if (failure != null) return Failed(chosen, failure)
            if (PushSync.steps(have).isEmpty() && have.serverId != null) return On(chosen)
            return Waiting(chosen)
        }
    }
}
