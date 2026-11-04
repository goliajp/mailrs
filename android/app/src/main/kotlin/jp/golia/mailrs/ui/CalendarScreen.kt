package jp.golia.mailrs.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import jp.golia.mailrs.CalendarPage
import jp.golia.mailrs.MailViewModel
import jp.golia.mailrs.R
import jp.golia.mailrs.UiState
import jp.golia.mailrs.closeCalendar
import jp.golia.mailrs.shiftCalendar
import jp.golia.mailrs.wire.CalendarWeek
import jp.golia.mailrs.wire.Texts
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/**
 * The reader's calendar, a week a page, read only: events are made and
 * answered on the web or from an invitation in a message.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CalendarScreen(state: UiState, page: CalendarPage, vm: MailViewModel) {
    val theme = LocalTheme.current
    val snackbars = remember { SnackbarHostState() }
    FailureSnackbar(state, vm, snackbars, hasContent = page.events != null)
    Scaffold(
        containerColor = theme.bg,
        snackbarHost = { SnackbarHost(snackbars) },
        topBar = {
            TopAppBar(
                title = { Text(Texts.get(R.string.calendar_title), fontSize = 17.sp, fontWeight = FontWeight.SemiBold) },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = theme.bg, titleContentColor = theme.fg),
                navigationIcon = {
                    IconButton(onClick = { vm.closeCalendar() }, modifier = Modifier.testTag("button.closeCalendar")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, Texts.get(R.string.common_back), tint = theme.fgSecondary)
                    }
                },
                actions = {
                    IconButton(onClick = { vm.shiftCalendar(-1) }, modifier = Modifier.testTag("calendar.previous")) {
                        Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, Texts.get(R.string.calendar_previous), tint = theme.fgSecondary)
                    }
                    IconButton(onClick = { vm.shiftCalendar(1) }, modifier = Modifier.testTag("calendar.next")) {
                        Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, Texts.get(R.string.calendar_next), tint = theme.fgSecondary)
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            val events = page.events
            if (events == null) {
                CircularProgressIndicator(Modifier.align(Alignment.Center), color = theme.accent)
            } else {
                WeekList(page.monday, events)
            }
        }
    }
}

@Composable
private fun WeekList(monday: LocalDate, events: List<CalendarWeek.Event>) {
    val theme = LocalTheme.current
    val zone = ZoneId.systemDefault()
    val days = CalendarWeek.days(monday)
    LazyColumn(Modifier.fillMaxSize().testTag("calendar.week.$monday")) {
        if (events.isEmpty()) {
            item {
                Text(
                    Texts.get(R.string.calendar_empty),
                    color = theme.fgMuted,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(16.dp).testTag("calendar.empty"),
                )
            }
        }
        items(days, key = { it.toString() }) { day ->
            val on = CalendarWeek.eventsOn(events, day, zone)
            Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("calendar.day.$day")) {
                Text(
                    day.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL)),
                    color = dayColor(day == LocalDate.now()),
                    fontSize = 13.sp,
                    fontWeight = FontWeight.SemiBold,
                )
                for (ev in on) {
                    EventLine(ev, day, zone)
                }
            }
            HorizontalDivider(color = theme.border, thickness = 0.5.dp)
        }
    }
}

@Composable
private fun EventLine(ev: CalendarWeek.Event, day: LocalDate, zone: ZoneId) {
    val theme = LocalTheme.current
    val span = CalendarWeek.span(ev, zone) ?: return
    Row(
        Modifier.fillMaxWidth().padding(top = 4.dp).testTag("calendar.event.${ev.uid}.$day"),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(whenLabel(span, zone), color = theme.fgMuted, fontSize = 12.sp)
        Text(ev.summary.orEmpty(), color = theme.fg, fontSize = 14.sp)
    }
}

/** "All day", or the start time in the reader's zone. */
private fun whenLabel(span: CalendarWeek.Span, zone: ZoneId): String {
    if (span.allDay) return Texts.get(R.string.calendar_all_day)
    return timeOf(span.start, zone)
}

private fun timeOf(instant: Instant, zone: ZoneId): String =
    instant.atZone(zone).toLocalTime().format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))

@Composable
private fun dayColor(today: Boolean): androidx.compose.ui.graphics.Color {
    if (today) return LocalTheme.current.accent
    return LocalTheme.current.fg
}
