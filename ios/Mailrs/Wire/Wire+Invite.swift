import Foundation

extension Wire {
    /// A meeting invitation, as the server read it out of the message.
    ///
    /// Backend: `crates/webapi/src/handlers/complete.rs` —
    /// `get_message_single`'s `invite_payload`, which is
    /// `mailrs_ical::ParsedInvite` plus the two resolved instants, and
    /// what `calendar_events::series::annotate` adds when it is read.
    struct Invite: Decodable, Sendable {
        let uid: String
        /// Higher on every re-send. Exchange does not send
        /// `METHOD:UPDATE` — it re-sends the whole invitation as a
        /// `REQUEST` with a higher sequence — so this is what tells an
        /// update from a first invitation.
        let sequence: Int
        let summary: String
        let location: String?
        let organizer: Person?
        let attendees: [Attendee]
        let status: String?
        /// **The instant, resolved on the server** against the
        /// invitation's own `VTIMEZONE`.
        ///
        /// Use this, not the wall-clock. A `TZID` is routinely a Windows
        /// name — `Pacific Standard Time`, which says "Standard" while
        /// the event is in daylight time — and no client-side date
        /// parser can evaluate one. `nil` for an all-day event, which
        /// has no instant: a date has no offset, and giving it one
        /// moves it a day.
        let startsAt: Date?
        let endsAt: Date?
        /// An all-day event's first day, `YYYY-MM-DD`, and the day after
        /// its last (DTEND, as RFC 5545 writes it). Absent for a timed
        /// event, which has `startsAt` instead.
        let firstDay: String?
        let dayAfterLast: String?
        /// The way into the meeting, resolved on the server: RFC 5545
        /// has no field for it, so Teams writes it into the description
        /// and Zoom into the location, and one implementation of "which
        /// URL is a meeting" beats three.
        let joinURL: URL?
        /// The zone the organiser wrote the time in, to show beside the
        /// reader's own when they differ.
        let organiserZone: String?
        /// The wall-clock the organiser wrote, for that same line.
        let organiserWallClock: String?
        /// The IANA zone the organiser's `TZID` names, Windows names
        /// resolved on the server. The organiser's clock is formatted from
        /// the instant in it, so the abbreviation is the one in force on
        /// that date — PDT in October, PST in November.
        let organiserIana: String?
        /// How a recurring invitation repeats; nil for one that does not.
        let rule: InviteRepeat.Rule?
        /// The first occurrence that has not ended, worked out by the
        /// server when the message is read. Nil for a single event, and
        /// for a series that is over (then `rule` is still set).
        let next: InviteRepeat.Occurrence?

        struct Person: Decodable, Sendable {
            let cn: String?
            let email: String
        }

        struct Attendee: Decodable, Sendable, Identifiable {
            let cn: String?
            let email: String
            /// `NEEDS-ACTION` / `ACCEPTED` / `DECLINED` / `TENTATIVE`.
            let partstat: String
            var id: String { email }

            enum CodingKeys: String, CodingKey {
                case cn
                case email
                case partstat
            }

            init(from decoder: Decoder) throws {
                let c = try decoder.container(keyedBy: CodingKeys.self)
                cn = try c.decodeIfPresent(String.self, forKey: .cn)
                email = try c.decode(String.self, forKey: .email)
                partstat = try c.decodeIfPresent(String.self, forKey: .partstat) ?? "NEEDS-ACTION"
            }
        }

        enum CodingKeys: String, CodingKey {
            case uid
            case sequence
            case summary
            case location
            case organizer
            case attendees
            case status
            case joinURL = "join_url"
            case dtstartUtc = "dtstart_utc"
            case dtendUtc = "dtend_utc"
            case dtstartDate = "dtstart_date"
            case dtendDate = "dtend_date"
            case dtstart
            case organiserIana = "organiser_zone"
            case rule = "recurrence"
            case next = "next_occurrence"
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            uid = try c.decodeIfPresent(String.self, forKey: .uid) ?? ""
            sequence = try c.decodeIfPresent(Int.self, forKey: .sequence) ?? 0
            summary = try c.decodeIfPresent(String.self, forKey: .summary) ?? ""
            location = try c.decodeIfPresent(String.self, forKey: .location)
            organizer = try c.decodeIfPresent(Person.self, forKey: .organizer)
            attendees = try c.decodeIfPresent([Attendee].self, forKey: .attendees) ?? []
            status = try c.decodeIfPresent(String.self, forKey: .status)
            joinURL = (try c.decodeIfPresent(String.self, forKey: .joinURL)).flatMap(URL.init)
            startsAt = Wire.Invite.instant(try c.decodeIfPresent(String.self, forKey: .dtstartUtc))
            endsAt = Wire.Invite.instant(try c.decodeIfPresent(String.self, forKey: .dtendUtc))
            firstDay = try c.decodeIfPresent(String.self, forKey: .dtstartDate)
            dayAfterLast = try c.decodeIfPresent(String.self, forKey: .dtendDate)
            // The original wall-clock and its zone, for the second line.
            // A `Zoned` date-time serialises as
            // `{"Zoned": {"local": …, "tz_name": …}}`.
            // `try?` already collapses the throwing half, so this is
            // one optional and not two: a `Date` or `Floating` start
            // simply does not decode as `Zoned`, which is not an error.
            let zoned = (try? c.decodeIfPresent(Zoned.self, forKey: .dtstart)) ?? nil
            organiserZone = zoned?.Zoned.tzName
            organiserWallClock = zoned?.Zoned.local
            organiserIana = try c.decodeIfPresent(String.self, forKey: .organiserIana)
            rule = try c.decodeIfPresent(InviteRepeat.Rule.self, forKey: .rule)
            next = try c.decodeIfPresent(InviteRepeat.Occurrence.self, forKey: .next)
        }

        private struct Zoned: Decodable, Sendable {
            struct Inner: Decodable, Sendable {
                let local: String
                let tzName: String

                enum CodingKeys: String, CodingKey {
                    case local
                    case tzName = "tz_name"
                }
            }

            // swiftlint:disable:next identifier_name
            let Zoned: Inner
        }

        private static func instant(_ rfc3339: String?) -> Date? {
            guard let rfc3339 else { return nil }
            let f = ISO8601DateFormatter()
            f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            return f.date(from: rfc3339) ?? {
                let g = ISO8601DateFormatter()
                g.formatOptions = [.withInternetDateTime]
                return g.date(from: rfc3339)
            }()
        }
    }

    /// A date somebody wrote in the body, offered as an event.
    ///
    /// Most mail about a meeting is not an invitation: no calendar
    /// part, no UID, nothing to accept — just a sentence with a time in
    /// The single-message read, of which this client wants the
    /// invitation and the reader's own answer.
    struct MessageDetail: Decodable, Sendable {
        let inviteMethod: String
        let invite: Invite?
        /// `ACCEPTED` / `TENTATIVE` / `DECLINED`, or nil when this
        /// reader has not answered.
        let rsvpStatus: String?

        enum CodingKeys: String, CodingKey {
            case inviteMethod = "invite_method"
            case invite = "invite_payload"
            case rsvpStatus = "rsvp_status"
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            inviteMethod = try c.decodeIfPresent(String.self, forKey: .inviteMethod) ?? ""
            invite = try? c.decodeIfPresent(Invite.self, forKey: .invite)
            rsvpStatus = try c.decodeIfPresent(String.self, forKey: .rsvpStatus)
        }
    }
}
