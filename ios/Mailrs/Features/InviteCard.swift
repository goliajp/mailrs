import SwiftUI

/// A meeting, above the mail that carries it.
///
/// The message arrives as a wall of HTML with a join link buried in it;
/// the thing a person needs — when, where, and whether to say yes — is
/// in the `text/calendar` part nobody was reading until 2026-08-20.
///
/// Times come from the server already resolved. A `TZID` is routinely a
/// Windows name like `Pacific Standard Time`, which says "Standard"
/// while the event is in daylight time, and no client-side parser can
/// evaluate one — the web read the wall-clock as UTC for two years and
/// showed a Santa Clara afternoon at one in the morning in Tokyo.
struct InviteCard: View {
    let uid: UInt32
    let method: String
    @Environment(Session.self) private var session
    @Environment(\.theme) private var theme
    @State private var detail: Wire.MessageDetail?
    @State private var sending = false
    @State private var failure: String?

    var body: some View {
        // A `VStack`, not a `Group`: a `Group` whose branches are all
        // false produces nothing at all, and a `.task` attached to
        // nothing never runs. The card mounted, the field said
        // REQUEST, and the fetch never happened — which looked exactly
        // like a message carrying no invitation.
        VStack(alignment: .leading, spacing: 0) {
            if let invite = detail?.invite {
                card(invite)
            } else if let failure {
                Label(failure, systemImage: "exclamationmark.triangle")
                    .font(.caption2)
                    .foregroundStyle(theme.danger)
                    .accessibilityIdentifier("invite.failure")
            }
        }
        .task(id: uid) {
            // Said, not swallowed. `try?` here meant a card that failed
            // to load looked exactly like a message carrying no
            // invitation — the silence this whole change exists to
            // remove, reproduced in the code that removes it.
            do {
                detail = try await session.client?.invite(uid: uid)
            } catch {
                failure = error.localizedDescription
            }
        }
    }

    @ViewBuilder private func card(_ invite: Wire.Invite) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Image(systemName: cancelled ? "calendar.badge.minus" : "calendar")
                Text(badge)
                    .font(.caption2.weight(.semibold))
                Spacer(minLength: 0)
            }
            .foregroundStyle(cancelled ? theme.danger : theme.accent)

            Text(invite.summary)
                .font(.subheadline.weight(.semibold))
                .strikethrough(cancelled)

            if let when = whenLine(invite) {
                Label(when, systemImage: "clock")
                    .font(.caption)
                    .foregroundStyle(theme.fgSecondary)
            }
            if let place = invite.location, !place.isEmpty {
                Label(place, systemImage: "mappin.and.ellipse")
                    .font(.caption)
                    .foregroundStyle(theme.fgSecondary)
                    .lineLimit(2)
            }
            // The way in, which is the most-used thing on a meeting
            // invitation and was missing until somebody looked at the
            // card instead of asserting about it.
            if let join = invite.joinURL, !cancelled {
                Link(destination: join) {
                    Label("Join the meeting", systemImage: "video")
                        .font(.caption.weight(.medium))
                }
                .accessibilityIdentifier("invite.join")
            }
            if let organizer = invite.organizer {
                Text("From \(organizer.cn ?? organizer.email)")
                    .font(.caption2)
                    .foregroundStyle(theme.fgMuted)
            }
            if !invite.attendees.isEmpty {
                Text(InviteGuests.summary(invite.attendees))
                    .font(.caption2)
                    .foregroundStyle(theme.fgMuted)
            }

            if let answered = detail?.rsvpStatus, !answered.isEmpty {
                Text(InviteGuests.answered(answered))
                    .font(.caption.weight(.medium))
                    .foregroundStyle(theme.accent)
            } else if InviteMethod.wantsAnswer(method), !cancelled {
                answerButtons
            }
            if let failure {
                Text(failure)
                    .font(.caption2)
                    .foregroundStyle(theme.danger)
            }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(theme.surface, in: RoundedRectangle(cornerRadius: 10))
        .overlay(
            RoundedRectangle(cornerRadius: 10).stroke(theme.border, lineWidth: 0.5)
        )
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("invite.card")
    }

    private var answerButtons: some View {
        HStack(spacing: 8) {
            answer("Yes", "ACCEPTED", .green)
            answer("Maybe", "TENTATIVE", .orange)
            answer("No", "DECLINED", .red)
        }
        .disabled(sending)
    }

    private func answer(_ label: LocalizedStringKey, _ partstat: String, _ tint: Color)
        -> some View
    {
        Button(label) {
            Task {
                sending = true
                failure = nil
                do {
                    try await session.client?.rsvp(uid: uid, partstat: partstat)
                    detail = try? await session.client?.invite(uid: uid)
                } catch {
                    // Said, not swallowed: an answer that did not reach
                    // the organiser leaves them waiting, and a card that
                    // shows "accepted" anyway is the failure this whole
                    // change is about.
                    failure = error.localizedDescription
                }
                sending = false
            }
        }
        .font(.caption.weight(.medium))
        .buttonStyle(.bordered)
        .tint(tint)
        .accessibilityIdentifier("invite.\(partstat.lowercased())")
    }

    private var cancelled: Bool { method.uppercased() == "CANCEL" }

    private var badge: LocalizedStringKey {
        InviteMethod.badge(method, sequence: detail?.invite?.sequence ?? 0)
    }

    /// The reader's own time, and the organiser's beside it when they
    /// differ — the second is what somebody joining across an ocean
    /// checks.
    private func whenLine(_ invite: Wire.Invite) -> String? {
        guard let starts = invite.startsAt else {
            // An all-day event has no instant and must not be given one.
            return nil
        }
        let local = starts.formatted(date: .abbreviated, time: .shortened)
        guard
            let zone = invite.organiserZone,
            let wall = invite.organiserWallClock,
            InviteMethod.zoneDiffers(zone)
        else {
            return local
        }
        let hhmm = String(wall.dropFirst(11).prefix(5))
        return "\(local) · \(hhmm) \(zone)"
    }
}
