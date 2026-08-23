package jp.golia.mailrs.accounts

/**
 * Whether to fetch a whole message or only its beginning.
 *
 * Opening a message costs what the message weighs. One with a 25 MB
 * attachment is 25 MB to fetch, and fetching it to show two lines of
 * text — on somebody's mobile data, without asking — is the kind of
 * thing a person notices on their bill rather than on their screen.
 *
 * Pure, because the decision is the part worth arguing with, and
 * because the **honesty rule** below is easy to drop: a client that
 * fetches part of a message and does not say so shows a message with
 * its attachments missing and no explanation.
 */
object FetchWhole {
    /** Past this, ask before fetching everything. */
    const val THRESHOLD = 1_000_000L

    /** How much of a large message to take on first open. */
    const val PREVIEW = 262_144L

    sealed interface Plan {
        /** Small enough, or the reader asked for all of it. */
        data object Whole : Plan

        /**
         * The first [bytes] only.
         *
         * **The caller must say so.** The text will usually be
         * complete — it comes before the attachments in nearly every
         * message — but the attachment list will not be, and a list
         * that is silently short is worse than one that is absent.
         */
        data class Beginning(val bytes: Long) : Plan
    }

    /**
     * @param size what the server said the message weighs, or null
     *   when it did not say. **Null fetches whole**: a message of
     *   unknown size is usually a small one, and refusing to show it
     *   properly on a guess is worse than the fetch.
     * @param askedForAll set when the reader has pressed the button.
     */
    fun decide(size: Long?, askedForAll: Boolean = false): Plan = when {
        askedForAll -> Plan.Whole
        size == null -> Plan.Whole
        size <= THRESHOLD -> Plan.Whole
        else -> Plan.Beginning(PREVIEW)
    }

    /**
     * The `BODY.PEEK[]` argument for a plan.
     *
     * `<0.262144>` is RFC 3501's partial fetch: offset then length.
     * The offset is written even though it is zero, because the form
     * without it means something else — the whole body.
     */
    fun bodyItem(plan: Plan): String = when (plan) {
        is Plan.Whole -> "BODY.PEEK[]"
        is Plan.Beginning -> "BODY.PEEK[]<0.${plan.bytes}>"
    }
}
