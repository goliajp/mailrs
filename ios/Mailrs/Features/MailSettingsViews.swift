import SwiftUI

/// The vacation reply, on or off. Its text and dates are written on the
/// web; turning it on without text is refused, so the switch waits for it.
struct VacationView: View {
    @Environment(Session.self) private var session
    @State private var vacation: SettingsWire.Vacation?

    var body: some View {
        Form {
            if let vacation {
                Section {
                    Toggle("Reply while I'm away", isOn: enabled)
                        .disabled(!vacation.canTurnOn && !vacation.enabled)
                        .accessibilityIdentifier("vacation.enabled")
                } footer: {
                    VacationFooter(vacation: vacation)
                }
                if vacation.canTurnOn {
                    Section("Reply") {
                        if !vacation.subject.isEmpty { Text(vacation.subject).font(.headline) }
                        Text(vacation.body).foregroundStyle(.secondary)
                    }
                }
            }
        }
        .navigationTitle("Vacation responder")
        .task { vacation = await session.vacation() }
    }

    /// The whole reply as read, with only `enabled` changed.
    private var enabled: Binding<Bool> {
        Binding(
            get: { vacation?.enabled ?? false },
            set: { on in
                guard var next = vacation else { return }
                next.enabled = on
                Task {
                    if let saved = await session.saveVacation(next) { vacation = saved }
                }
            })
    }
}

private struct VacationFooter: View {
    let vacation: SettingsWire.Vacation

    var body: some View {
        if vacation.canTurnOn {
            Text("One reply per sender every \(vacation.days) days")
        } else {
            Text("Write the reply on the web first")
                .accessibilityIdentifier("vacation.needsText")
        }
    }
}

/// Addresses that forward here and can be turned off once they leak.
struct MaskedAddressesView: View {
    @Environment(Session.self) private var session
    @State private var masks: [SettingsWire.Mask]?
    @State private var forDomain = ""

    var body: some View {
        List {
            Section {
                TextField("For which site", text: $forDomain)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .accessibilityIdentifier("masked.forDomain")
                Button("New masked address") { Task { await create() } }
                    .accessibilityIdentifier("masked.create")
            }
            if let masks {
                Section {
                    if masks.isEmpty { Text("No masked addresses").foregroundStyle(.secondary) }
                    ForEach(masks) { mask in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(mask.email)
                            Text(MaskedAddressesView.note(mask)).font(.caption).foregroundStyle(.secondary)
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("mask.\(mask.id)")
                    }
                }
            }
        }
        .navigationTitle("Masked addresses")
        .task { masks = await session.masks() }
    }

    private func create() async {
        let site = forDomain.trimmingCharacters(in: .whitespaces)
        guard let made = await session.createMask(forDomain: site, description: "") else { return }
        forDomain = ""
        masks = [made] + (masks ?? [])
    }

    /// The site it was made for, else its description.
    static func note(_ mask: SettingsWire.Mask) -> String {
        if let site = mask.forDomain, !site.isEmpty { return site }
        return mask.description ?? ""
    }
}
