package jp.golia.mailrs

import androidx.lifecycle.viewModelScope
import jp.golia.mailrs.wire.CalendarWeek
import jp.golia.mailrs.wire.MailrsClient
import jp.golia.mailrs.wire.calendarEvents
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.LocalDate
import java.time.ZoneId

/** The calendar page while it is open: which week, and its events once read. */
data class CalendarPage(
    val monday: LocalDate,
    val events: List<CalendarWeek.Event>? = null,
)

/** Open on the week holding today. */
fun MailViewModel.openCalendar() {
    showWeek(CalendarWeek.startOfWeek(LocalDate.now()))
}

fun MailViewModel.closeCalendar() {
    _state.update { it.copy(calendar = null) }
}

/** One week back (`-1`) or forward (`1`). */
fun MailViewModel.shiftCalendar(step: Long) {
    val monday = _state.value.calendar?.monday ?: return
    showWeek(monday.plusWeeks(step))
}

private fun MailViewModel.showWeek(monday: LocalDate) {
    _state.update { it.copy(calendar = CalendarPage(monday), error = null) }
    viewModelScope.launch {
        when (val r = client.calendarEvents(monday, ZoneId.systemDefault())) {
            is MailrsClient.Outcome.Ok -> _state.update { s ->
                // a later week asked for meanwhile wins
                if (s.calendar?.monday != monday) return@update s
                s.copy(calendar = CalendarPage(monday, r.value))
            }
            is MailrsClient.Outcome.Err -> _state.update { it.copy(error = r.message) }
        }
    }
}
