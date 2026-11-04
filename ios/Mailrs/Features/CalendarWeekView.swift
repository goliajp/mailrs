import SwiftUI

/// The reader's calendar, a week a page, read only: events are made and
/// answered on the web or from an invitation in a message.
struct CalendarWeekView: View {
    @Environment(Session.self) private var session
    @Environment(\.dismiss) private var dismiss
    @Environment(\.calendar) private var calendar
    @State private var monday: Date?
    @State private var events: [CalendarWeek.Event]?

    var body: some View {
        NavigationStack {
            Group {
                if let monday, let events {
                    WeekList(monday: monday, events: events)
                } else {
                    ProgressView()
                }
            }
            .navigationTitle("Calendar")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItemGroup(placement: .primaryAction) {
                    Button { shift(-1) } label: { Label("Previous week", systemImage: "chevron.left") }
                        .accessibilityIdentifier("calendar.previous")
                    Button { shift(1) } label: { Label("Next week", systemImage: "chevron.right") }
                        .accessibilityIdentifier("calendar.next")
                }
            }
        }
        .task(id: monday) { await load() }
    }

    private func shift(_ weeks: Int) {
        guard let monday else { return }
        self.monday = calendar.date(byAdding: .day, value: 7 * weeks, to: monday)
    }

    private func load() async {
        guard let week = monday else {
            monday = CalendarWeek.startOfWeek(Date(), calendar: calendar)
            return
        }
        events = nil
        let read = await session.calendarWeek(monday: week, calendar: calendar)
        // a later week asked for meanwhile wins
        if week == monday { events = read }
    }
}

private struct WeekList: View {
    @Environment(\.calendar) private var calendar
    let monday: Date
    let events: [CalendarWeek.Event]

    var body: some View {
        List {
            if events.isEmpty {
                Text("No events this week")
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("calendar.empty")
            }
            ForEach(CalendarWeek.days(from: monday, calendar: calendar), id: \.self) { day in
                let key = CalendarWeek.dayKey(day, calendar: calendar)
                Section {
                    ForEach(CalendarWeek.events(events, on: day, calendar: calendar)) { ev in
                        EventLine(event: ev)
                            .accessibilityIdentifier("calendar.event.\(ev.uid).\(key)")
                    }
                } header: {
                    Text(day, format: .dateTime.weekday(.wide).month().day())
                        .foregroundStyle(DayTone.color(today: calendar.isDateInToday(day)))
                        .accessibilityIdentifier("calendar.day.\(key)")
                }
            }
        }
    }
}

private struct EventLine: View {
    @Environment(\.calendar) private var calendar
    let event: CalendarWeek.Event

    var body: some View {
        HStack(spacing: 8) {
            if let span = CalendarWeek.span(event, calendar: calendar) {
                if span.allDay {
                    Text("All day").font(.caption).foregroundStyle(.secondary)
                } else {
                    Text(span.start, format: .dateTime.hour().minute()).font(.caption).foregroundStyle(.secondary)
                }
            }
            Text(event.summary ?? "")
        }
        .accessibilityElement(children: .combine)
    }
}

private enum DayTone {
    static func color(today: Bool) -> Color {
        if today { return .accentColor }
        return .primary
    }
}
