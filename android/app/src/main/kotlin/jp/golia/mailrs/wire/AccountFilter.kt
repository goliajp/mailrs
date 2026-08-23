package jp.golia.mailrs.wire

/** One row of the account filter. */
data class FilterRow(val id: String, val label: String, val colour: String?)

/**
 * Narrowing the one list to some of the connected mailboxes.
 *
 * A **filter, not a switcher**: every box starts ticked and unticking
 * one takes that account out. Somebody with work, personal and two
 * others wants the first two together, which "only this" cannot say.
 *
 * The empty id is this deployment's own mail — a row like the rest, so
 * it can be switched off too.
 */
fun filterRows(own: String, accounts: List<ExternalAccount>): List<FilterRow> {
    val rows = mutableListOf(FilterRow("", own.ifEmpty { "This server" }, null))
    for (a in accounts) {
        rows += FilterRow(a.id, a.displayName.ifEmpty { a.email }, a.colour)
    }
    return rows
}

/**
 * The ids to ask for after ticking or unticking one.
 *
 * `null` means no filter at all: back to everything is not "every id
 * in the parameter", it is the parameter absent. A request carrying
 * every id would narrow to exactly the same set and cost a longer URL
 * to say nothing — and the two would be indistinguishable in a log.
 *
 * Unticking the last one is refused rather than sending an empty
 * filter: a list narrowed to no accounts is a blank screen with no way
 * back except the control that produced it.
 */
fun toggledAccounts(selected: List<String>?, all: List<String>, id: String): List<String>? {
    val on = selected ?: all
    val next = if (on.contains(id)) on.filterNot { it == id } else on + id
    return when {
        next.isEmpty() -> selected
        next.size == all.size -> null
        else -> next
    }
}

/** What the control says it is doing. */
fun filterLabel(selected: List<String>?, all: List<String>): String = when {
    selected == null || selected.size == all.size -> "All accounts"
    else -> "${selected.size} of ${all.size} accounts"
}
