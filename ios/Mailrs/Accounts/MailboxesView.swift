import SwiftUI

/// One list for every connected mailbox.
///
/// The shape every working mail client settles on: one list by
/// default, and a way to narrow it. Narrowing is a row of chips rather
/// than a menu, because the useful question — "is anything in here
/// from work?" — is answered by looking, not by opening something.
struct MailboxesView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.theme) private var theme
    @State private var model = MailboxesModel()
    /// The message being read, if any.
    @State private var opened: MailboxRow?
    /// The message being written, if any, and which account it leaves
    /// by.
    @State private var writing: Writing?

    struct Writing: Identifiable {
        let id = UUID()
        let draft: OutgoingMessage.Draft
        let accountId: String
    }

    var body: some View {
        NavigationStack {
            Group {
                if model.accounts.isEmpty {
                    empty
                } else {
                    list
                }
            }
            .navigationTitle("Mailboxes")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("mailboxes.done")
                }
                ToolbarItem(placement: .primaryAction) {
                    // Writing does not depend on having fetched
                    // anything, so it is offered as soon as there is an
                    // account to send from.
                    //
                    // **Not `.secondaryAction`.** That places it in the
                    // toolbar's overflow menu, and writing a message is
                    // the one thing a mail app must never hide behind a
                    // "…" — no mainstream client does. It was found by
                    // a UI test that could not reach the composer at
                    // all and reported "there was no way to write a
                    // message", which turned out to be a fair
                    // description of the screen.
                    if let account = model.accounts.first(where: { model.only.contains($0.id) })
                        ?? model.accounts.first
                    {
                        Button {
                            writing = Writing(
                                draft: OutgoingMessage.Draft(
                                    from: account.address, fromName: account.displayName, to: []),
                                accountId: account.id)
                        } label: {
                            Image(systemName: "square.and.pencil")
                        }
                        .accessibilityLabel("New message")
                        .accessibilityIdentifier("mail.compose")
                    }
                }
                ToolbarItem(placement: .primaryAction) {
                    if model.syncing {
                        ProgressView()
                    } else {
                        Button {
                            Task { await model.sync() }
                        } label: {
                            Image(systemName: "arrow.clockwise")
                        }
                        .accessibilityLabel("Fetch mail")
                        .accessibilityIdentifier("mailboxes.sync")
                    }
                }
            }
        }
        .task { model.load() }
        .sheet(item: $opened) { row in
            MessageView(row: row, account: model.account(for: row)) { loaded, all in
                // The reply leaves by the account the message arrived
                // at. Replying from a different address than the one
                // that was written to is a mistake nobody notices
                // until the answer goes missing.
                guard let account = model.account(for: row) else { return }
                writing = Writing(
                    draft: ReplyDraft.make(
                        to: loaded.headers, from: account, quoting: loaded.text, all: all),
                    accountId: account.id)
                opened = nil
            }
        }
        .sheet(item: $writing) { writing in
            ComposeMailView(
                accounts: model.accounts, initial: writing.draft,
                initialAccountId: writing.accountId)
        }
        // Reading marks a message read on the server and on this
        // device; the list has to be told, or it goes on showing it as
        // unread until the next fetch.
        .onChange(of: opened) { _, now in
            if now == nil { model.load() }
        }
    }

    /// Nothing to show, and the two reasons differ: no mail at all is
    /// a prompt to fetch, while nothing from the chosen mailboxes is a
    /// prompt to widen the filter.
    private var emptyListText: LocalizedStringKey {
        // Three different nothings, and each leads somewhere different:
        // fetch, widen the filter, or know that a local search cannot
        // see what was never fetched.
        if !model.query.isEmpty {
            return "Nothing here matches. Only mail already fetched is searched."
        }
        if model.only.isEmpty { return "No mail yet. Pull to fetch." }
        return "Nothing from the mailboxes you picked."
    }

    private func chipBackground(on: Bool) -> Color {
        if on { return theme.accent.opacity(0.18) }
        return theme.bgSecondary
    }

    private func chipTraits(on: Bool) -> AccessibilityTraits {
        if on { return .isSelected }
        return []
    }

    private var empty: some View {
        ContentUnavailableView {
            Label("No mailboxes yet", systemImage: "tray")
        } description: {
            Text("Add one in Settings to read it here.")
        }
        .accessibilityIdentifier("mailboxes.empty")
    }

    private var list: some View {
        List {
            if model.accounts.count > 1 { filters }
            unreachable
            if model.visible.isEmpty {
                Text(emptyListText)
                    .font(.footnote)
                    .foregroundStyle(theme.fgMuted)
                    .accessibilityIdentifier("mailboxes.nothing")
            }
            ForEach(model.visible) { row in
                Button {
                    opened = row
                } label: {
                    MailboxRowView(row: row, account: model.account(for: row))
                }
                .buttonStyle(.plain)
                // The gesture every mail client uses, and the two
                // directions are chosen the way they are everywhere:
                // the destructive one is the one nobody reaches for by
                // accident.
                .swipeActions(edge: .trailing) {
                    Button(role: .destructive) {
                        Task { await model.delete(row) }
                    } label: {
                        Label("Delete", systemImage: "trash")
                    }
                }
                .swipeActions(edge: .leading) {
                    Button {
                        Task { await model.markUnread(row) }
                    } label: {
                        Label("Unread", systemImage: "envelope.badge")
                    }
                    .tint(.blue)
                }
            }
            // Show more of what is already here before asking the
            // server for more. The window grows by a page when the
            // list reaches its end, and only once it has stopped
            // growing — the store had no more to give — is there
            // anything to fetch.
            //
            // Both used to be one button, and it read as one action;
            // it was not, and the slow one ran when the fast one would
            // have done.
            if model.moreHeld {
                HStack {
                    Spacer()
                    ProgressView()
                    Spacer()
                }
                .onAppear { model.showMore() }
            }
            // At the end of the list, which is where somebody reaches
            // it by scrolling — and only when nothing is being
            // searched for, because "earlier" against a filtered list
            // fetches mail that will not be shown.
            if MailboxWindow.offersEarlier(
                moreHeld: model.moreHeld, shownCount: model.visible.count,
                searching: !model.query.isEmpty)
            {
                HStack {
                    Spacer()
                    if model.reaching {
                        ProgressView()
                    } else {
                        Button("Fetch earlier mail") {
                            Task { await model.fetchEarlier() }
                        }
                        .font(.footnote)
                        .accessibilityIdentifier("mail.earlier")
                    }
                    Spacer()
                }
            }
        }
        .listStyle(.plain)
        .refreshable { await model.sync() }
        .searchable(text: $model.query, prompt: "Search")
    }

    /// One chip per account. None selected is the ordinary state and
    /// means everything — a person who has picked nothing has not
    /// asked to be shown less.
    private var filters: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                let unread = model.unreadPerAccount
                ForEach(model.accounts) { account in
                    let on = model.only.contains(account.id)
                    Button {
                        model.toggle(account.id)
                    } label: {
                        HStack(spacing: 6) {
                            Circle()
                                .fill(Color(hex: AccountColour.forId(account.id)))
                                .frame(width: 8, height: 8)
                            Text(account.title).font(.footnote)
                            // Absent rather than `0`: a badge that says
                            // nothing while taking the space of one
                            // that would is worse than no badge.
                            if let count = unread[account.id] {
                                Text(String(count))
                                    .font(.caption2.weight(.semibold))
                                    .foregroundStyle(theme.accent)
                            }
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(chipBackground(on: on))
                        .clipShape(Capsule())
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("mailboxes.filter.\(account.address)")
                    .accessibilityAddTraits(chipTraits(on: on))
                }
            }
            .padding(.vertical, 4)
        }
        .listRowInsets(EdgeInsets(top: 4, leading: 12, bottom: 4, trailing: 12))
    }

    /// An account that could not be read says so here, by name.
    ///
    /// Not an alert: one unreachable server must not stand in front of
    /// the mail from the five that answered.
    @ViewBuilder private var unreachable: some View {
        ForEach(model.accounts.filter { model.failures[$0.id] != nil }) { account in
            HStack(spacing: 8) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                VStack(alignment: .leading, spacing: 2) {
                    Text(account.title).font(.footnote.weight(.medium))
                    Text(model.failures[account.id] ?? "")
                        .font(.caption)
                        .foregroundStyle(theme.fgMuted)
                }
            }
            .accessibilityIdentifier("mailboxes.failure.\(account.address)")
        }
    }
}

/// One message in the merged list.
struct MailboxRowView: View {
    let row: MailboxRow
    let account: MailAccount?
    @Environment(\.theme) private var theme

    /// Unread is heavier. The only thing on the row that says so
    /// without colour, which is why it is weight and not a tint.
    private var senderWeight: Font.Weight {
        if row.seen { return .regular }
        return .semibold
    }

    private var subjectColour: Color {
        if row.seen { return theme.fgMuted }
        return theme.fg
    }

    private var mailboxLine: String {
        var name = "Unknown"
        if let account { name = account.title }
        return "\(name) · \(row.folder)"
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Circle()
                .fill(Color(hex: AccountColour.forId(row.accountId)))
                .frame(width: 8, height: 8)
                .padding(.top, 6)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                HStack {
                    Text(row.displaySender)
                        .font(.subheadline.weight(senderWeight))
                        .lineLimit(1)
                    Spacer()
                    if let when = row.date {
                        Text(Date(timeIntervalSince1970: TimeInterval(when)),
                             format: .relative(presentation: .named))
                            .font(.caption)
                            .foregroundStyle(theme.fgMuted)
                    }
                }
                Text(row.displaySubject)
                    .font(.subheadline)
                    .foregroundStyle(subjectColour)
                    .lineLimit(2)
                // Which mailbox, in words. The dot is a shortcut for
                // people who can see it; this line is the answer for
                // everybody else.
                Text(mailboxLine)
                    .font(.caption2)
                    .foregroundStyle(theme.fgMuted)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("mailboxes.row.\(row.id)")
    }
}
