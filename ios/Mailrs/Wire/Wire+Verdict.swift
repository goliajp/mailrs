import Foundation

extension Wire {
    /// One reason a message was judged as it was.
    ///
    /// Backend: `mailrs_inbound::reason::Reason` (crates/inbound/src/reason.rs).
    /// `code` is a fraud rule id or one of that module's codes; `params`
    /// is left out when empty. `score` is set on a verdict layer's lines:
    /// what each added, so the layer's score is the sum of its lines;
    /// absent on a line that adds nothing, on placement, on older verdicts.
    struct Reason: Codable, Equatable, Hashable, Sendable {
        let code: String
        let params: [String: String]
        let score: Double?

        init(code: String, params: [String: String] = [:], score: Double? = nil) {
            self.code = code
            self.params = params
            self.score = score
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            code = try c.decode(String.self, forKey: .code)
            params = try c.decodeIfPresent([String: String].self, forKey: .params) ?? [:]
            score = try c.decodeIfPresent(Double.self, forKey: .score)
        }
    }

    /// A link worth a second look, by the host it goes to.
    ///
    /// Backend: `LinkWarning` in crates/webapi/src/handlers/message_reasons.rs.
    /// `shown` is left out when the link's text shows no other host.
    struct LinkWarning: Codable, Equatable, Hashable, Sendable {
        let host: String
        let shown: String?
        let reasons: [Reason]
    }

    /// One of the four layers of a stored fraud verdict.
    struct FraudLayer: Codable, Equatable, Sendable {
        let name: String
        /// `pass`, `fail` or `not_applicable`.
        let outcome: String
        /// English, for a code this client has no sentence for.
        let detail: String
        let reasons: [Reason]
        let score: Double

        init(name: String, outcome: String, detail: String, reasons: [Reason], score: Double) {
            self.name = name
            self.outcome = outcome
            self.detail = detail
            self.reasons = reasons
            self.score = score
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            name = try c.decode(String.self, forKey: .name)
            outcome = try c.decode(String.self, forKey: .outcome)
            detail = try c.decodeIfPresent(String.self, forKey: .detail) ?? ""
            // absent on verdicts stored before reasons existed
            reasons = try c.decodeIfPresent([Reason].self, forKey: .reasons) ?? []
            score = try c.decode(Double.self, forKey: .score)
        }
    }

    /// The stored fraud verdict for one message.
    ///
    /// Backend: `get_fraud_verdict` in crates/webapi/src/handlers/quarantine.rs,
    /// shape owned by `mailrs_inbound::verdict::FraudVerdict`.
    struct FraudVerdict: Codable, Equatable, Sendable {
        /// Empty on a record that exists only to say why mail nobody
        /// suspected was filed where it was.
        let layers: [FraudLayer]
        /// Why it was filed where it was: Junk, a sender rule, the
        /// classifier, a filter.
        let placement: [Reason]
        let quarantined: Bool
        let rulesVersion: String

        enum CodingKeys: String, CodingKey {
            case layers, placement, quarantined
            case rulesVersion = "rules_version"
        }

        init(layers: [FraudLayer], placement: [Reason], quarantined: Bool, rulesVersion: String) {
            self.layers = layers
            self.placement = placement
            self.quarantined = quarantined
            self.rulesVersion = rulesVersion
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            layers = try c.decode([FraudLayer].self, forKey: .layers)
            placement = try c.decodeIfPresent([Reason].self, forKey: .placement) ?? []
            quarantined = try c.decode(Bool.self, forKey: .quarantined)
            rulesVersion = try c.decode(String.self, forKey: .rulesVersion)
        }
    }

    /// `{"verdict": … | null}`. `null` is the ordinary answer: nobody
    /// suspected the message and nothing sorted it, which is not the
    /// same as "examined and cleared".
    struct FraudVerdictResponse: Decodable, Sendable {
        let verdict: FraudVerdict?
    }
}

extension MailrsClient {
    /// `GET /api/messages/{message_id}/fraud-verdict`.
    func fraudVerdict(messageId: String) async throws -> Wire.FraudVerdict? {
        let (data, response) = try await send(
            "GET", "/api/messages/\(MailrsClient.segment(messageId))/fraud-verdict",
            body: nil, authorized: true)
        try Self.ok(response)
        do {
            return try JSONDecoder().decode(Wire.FraudVerdictResponse.self, from: data).verdict
        } catch {
            throw MailrsError.decoding("fraud verdict — \(error)")
        }
    }
}
