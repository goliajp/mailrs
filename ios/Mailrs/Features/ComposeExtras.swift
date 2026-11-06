import SwiftUI

/// "Scheduled for …" under the header of a resumed draft that was set to
/// go later, with a way to take the time off and send at once.
struct ScheduledLine: View {
    let at: Int64
    let onClear: () -> Void
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "clock")
                .foregroundStyle(.secondary)
            Text("Scheduled for \(when)")
                .font(.footnote)
                .accessibilityIdentifier("composer-scheduled")
            Spacer(minLength: 0)
            Button("Send now instead", action: onClear)
                .font(.footnote)
                .accessibilityIdentifier("composer-unschedule")
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 6)
    }

    private var when: String {
        Date(timeIntervalSince1970: TimeInterval(at)).formatted(
            Date.FormatStyle(date: .abbreviated, time: .shortened, locale: locale, timeZone: timeZone))
    }
}

/// The account's templates, as the web composer's template list; absent
/// when there are none.
struct TemplateMenu: View {
    let templates: [Wire.Template]
    let onPick: (Wire.Template) -> Void

    var body: some View {
        if !templates.isEmpty {
            Menu {
                ForEach(templates) { template in
                    Button(template.name) { onPick(template) }
                }
            } label: {
                Label("Templates", systemImage: "doc.on.clipboard")
            }
            .accessibilityIdentifier("composer-templates")
        }
    }
}
