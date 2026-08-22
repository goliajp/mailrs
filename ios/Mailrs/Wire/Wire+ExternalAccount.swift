import Foundation

extension Wire {
    /// A mailbox somewhere else, as the server stores it.
    ///
    /// **There is no secret on it.** The password goes to the server
    /// once, sealed there, and no route returns it — not even to the
    /// person who typed it, who has it already and would only be
    /// putting it through another log.
    struct ExternalAccount: Decodable, Sendable, Identifiable {
        let id: String
        let email: String
        let displayName: String
        /// A preset id — `gmail`, `qq` — or `custom`.
        let provider: String
        /// `#rrggbb`, chosen by the server so all three clients agree
        /// on which dot means which mailbox.
        let colour: String?
        /// `ok` / `needs_auth` / `error` / `paused`.
        let state: String
        /// Why the last sync failed, for a row that is not `ok`.
        let lastError: String?

        enum CodingKeys: String, CodingKey {
            case id, email, provider, colour, state
            case displayName = "display_name"
            case lastError = "last_error"
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            id = try c.decode(String.self, forKey: .id)
            email = try c.decode(String.self, forKey: .email)
            displayName = try c.decodeIfPresent(String.self, forKey: .displayName) ?? ""
            provider = try c.decodeIfPresent(String.self, forKey: .provider) ?? "custom"
            colour = try? c.decodeIfPresent(String.self, forKey: .colour)
            // A row written before a state existed reads as working,
            // which is the same default the server takes.
            state = try c.decodeIfPresent(String.self, forKey: .state) ?? "ok"
            lastError = try? c.decodeIfPresent(String.self, forKey: .lastError)
        }

        /// What the row says on screen, and what a person can do about
        /// it. The two failures need different words: one is a button
        /// to press, the other is waiting.
        var trouble: String? {
            switch state {
            case "needs_auth": "Sign in again"
            case "error": "Not syncing"
            case "paused": "Paused"
            default: nil
            }
        }
    }

    /// What a set-up screen should fill in for an address.
    struct AccountSettings: Decodable, Sendable {
        let known: Bool
        let preset: Preset?

        struct Preset: Decodable, Sendable {
            let id: String
            let label: String
            /// `password` / `app_password` / `oauth2`.
            let auth: String
            let secretHelp: SecretHelp?

            enum CodingKeys: String, CodingKey {
                case id, label, auth
                case secretHelp = "secret_help"
            }
        }

        /// Where to get what this provider wants, in its own words.
        struct SecretHelp: Decodable, Sendable {
            /// Its name at the provider — "授权码", "App Password".
            let what: String
            /// The page that generates one.
            let url: String
        }
    }
}
