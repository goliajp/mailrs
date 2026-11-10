import Foundation
import Testing

@testable import Mailrs

@Suite("Reason text")
struct ReasonTextTests {
    private let table = ReasonText.bundled

    /// The copy shipped in the app is the server's table: a Rust test
    /// compares the bytes, this one proves the app carries and reads it.
    @Test("the bundled table has every code in three languages")
    func bundledTableIsComplete() {
        #expect(table.table.count > 30)
        for (code, entry) in table.table {
            #expect(!entry.en.isEmpty && !entry.ja.isEmpty && !entry.zh.isEmpty, "\(code)")
            if let p = entry.params {
                #expect(!p.en.isEmpty && !p.ja.isEmpty && !p.zh.isEmpty, "\(code) params")
            }
        }
    }

    @Test("a reason carrying every fact gets the sentence that names them")
    func paramsSentence() {
        let r = Wire.Reason(code: "claims-our-name", params: ["name": "Golia"])
        #expect(table.text(r, .en) == "The display name claims to be Golia.")
        #expect(table.text(r, .ja) == "表示名が「Golia」を名乗っています。")
        #expect(table.text(r, .zh) == "显示名自称是「Golia」。")
    }

    @Test("a reason stored before the rule named its facts gets the general sentence")
    func generalSentence() {
        let r = Wire.Reason(code: "bec-pay-to-a-stranger", params: ["domain": "x.example"])
        #expect(table.text(r, .en) == "The first message from this address carries a payment account.")
    }

    @Test("a category is named the way the reader knows it")
    func className() {
        let r = Wire.Reason(code: "sorted-by-classifier", params: ["class": "promotion"])
        #expect(table.text(r, .en)?.contains("Promotions") == true)
        #expect(table.text(r, .ja)?.contains("プロモーション") == true)
        #expect(table.text(r, .zh)?.contains("推广") == true)
    }

    @Test("SPF, DKIM and DMARC each say what they said")
    func authCheck() {
        let r = Wire.Reason(code: "auth-check", params: ["check": "DKIM", "result": "fail"])
        #expect(table.text(r, .en) == "DKIM: fail")
        #expect(table.text(r, .ja) == "DKIM：fail")
    }

    @Test("a code this build has no sentence for falls back to the code")
    func unknownCode() {
        let r = Wire.Reason(code: "rule-added-later")
        #expect(table.text(r, .en) == nil)
        #expect(table.words(r, .en) == "rule-added-later")
    }

    @Test("a value that reads like a placeholder is shown as written")
    func onePass() {
        let r = Wire.Reason(code: "link-shows-another-host", params: ["shown": "{host}", "host": "a.example"])
        #expect(table.text(r, .en) == "This link shows {host} but goes to a.example.")
    }

    @Test("the column follows the interface language")
    func language() {
        #expect(ReasonText.Lang.of(Locale(identifier: "ja_JP")) == .ja)
        #expect(ReasonText.Lang.of(Locale(identifier: "zh-Hans")) == .zh)
        #expect(ReasonText.Lang.of(Locale(identifier: "fr_FR")) == .en)
    }
}

@Suite("Fraud verdict wire")
@MainActor
struct FraudVerdictWireTests {
    /// The shape `GET /api/messages/{id}/fraud-verdict` answers on
    /// production, reasons with and without params.
    @Test("a stored verdict decodes")
    func decodes() throws {
        let json = """
        {"verdict":{"layers":[{"name":"identity","outcome":"fail","detail":"claims our name",
          "reasons":[{"code":"claims-our-name","params":{"name":"Golia"}}],"score":4.0},
          {"name":"transport","outcome":"pass","detail":"","score":0}],
          "placement":[{"code":"junk-your-filter"}],"quarantined":true,
          "rules_version":"2026-11-01.3","score":4.0}}
        """
        let v = try #require(try JSONDecoder().decode(
            Wire.FraudVerdictResponse.self, from: Data(json.utf8)).verdict)
        #expect(v.quarantined)
        #expect(v.rulesVersion == "2026-11-01.3")
        #expect(v.layers.map(\.name) == ["identity", "transport"])
        #expect(v.layers[0].reasons == [Wire.Reason(code: "claims-our-name", params: ["name": "Golia"])])
        #expect(v.layers[1].reasons.isEmpty)
        #expect(v.placement == [Wire.Reason(code: "junk-your-filter")])
    }

    /// A content layer as `mailrs_inbound::assess` stores it: the content
    /// scan as one line, each content rule as its own, each with what it
    /// added. The numbers the panel writes under the layer add up to the
    /// number it writes beside it.
    @Test("every content rule is a line, and the layer is their sum")
    func contentLines() throws {
        let json = """
        {"verdict":{"layers":[{"name":"transport","outcome":"pass","detail":"",
          "reasons":[{"code":"auth-check","params":{"check":"SPF","result":"pass"}}],"score":0},
          {"name":"content","outcome":"fail","detail":"d","score":12.5,
          "reasons":[{"code":"content-score","params":{"content":"1.5","classifier":"0.0","rules":""},"score":1.5},
          {"code":"offers-the-reader-a-sum","score":5},{"code":"asks-for-the-readers-line","score":6}]}],
          "quarantined":true,"rules_version":"2026-11-06.1","score":12.5}}
        """
        let v = try #require(try JSONDecoder().decode(
            Wire.FraudVerdictResponse.self, from: Data(json.utf8)).verdict)
        let content = try #require(v.layers.first { $0.name == "content" })
        #expect(content.reasons.map(\.code) == ["content-score", "offers-the-reader-a-sum", "asks-for-the-readers-line"])
        let shown = content.reasons.compactMap(\.score).map { Double(VerdictPanel.scoreLabel($0).dropFirst())! }
        #expect(shown.count == content.reasons.count)
        #expect(VerdictPanel.scoreLabel(shown.reduce(0, +)) == VerdictPanel.scoreLabel(content.score))
        // the authentication checks add nothing and carry no number
        #expect(v.layers[0].reasons.allSatisfy { $0.score == nil })
    }

    @Test("no verdict is null, not an error")
    func null() throws {
        let r = try JSONDecoder().decode(Wire.FraudVerdictResponse.self, from: Data(#"{"verdict":null}"#.utf8))
        #expect(r.verdict == nil)
    }

    @Test("a message carries its sender checks and link warnings")
    func messageFields() throws {
        let json = """
        {"uid":2,"sender":"x@y","recipients":"me","subject":"s","internal_date":1,"message_id":"<m>",
         "sender_checks":[{"code":"auth-check","params":{"check":"SPF","result":"pass"}}],
         "link_warnings":[{"host":"bad.example","shown":"www.golia.jp","reasons":[{"code":"link-host-listed","params":{"host":"bad.example"}}]}]}
        """
        let m = try JSONDecoder().decode(Wire.Message.self, from: Data(json.utf8))
        #expect(m.senderChecks.first?.params["check"] == "SPF")
        #expect(m.linkWarnings.first?.shown == "www.golia.jp")
        let old = try JSONDecoder().decode(Wire.Message.self, from: Data("""
        {"uid":2,"sender":"x@y","recipients":"me","subject":"s","internal_date":1,"message_id":"<m>"}
        """.utf8))
        #expect(old.senderChecks.isEmpty && old.linkWarnings.isEmpty)
    }
}

@Suite("Link warnings")
struct LinkWarningsTests {
    private let shows = Wire.LinkWarning(
        host: "webmail-oauth.wzldq.com", shown: "www.golia.jp",
        reasons: [Wire.Reason(code: "link-shows-another-host")])
    private let listed = Wire.LinkWarning(
        host: "bad.example", shown: nil, reasons: [Wire.Reason(code: "link-host-listed")])

    @Test("an anchor showing another host is matched by its text")
    func matchesShown() {
        let w = LinkWarnings.warning(
            href: "https://webmail-oauth.wzldq.com/auth", text: "www.golia.jp/ lihao/check",
            in: [shows])
        #expect(w == shows)
        // same host, text that shows nothing else: not this warning
        #expect(LinkWarnings.warning(
            href: "https://webmail-oauth.wzldq.com/auth", text: "sign in", in: [shows]) == nil)
    }

    @Test("a listed host is matched whatever the text")
    func matchesListed() {
        #expect(LinkWarnings.warning(href: "https://BAD.example/x", text: "here", in: [listed]) == listed)
        #expect(LinkWarnings.warning(href: "https://fine.example/", text: "here", in: [listed]) == nil)
    }

    @Test("a flagged anchor is pointed at the question, with a mark after it")
    func marks() throws {
        let html = #"<p><a href="https://webmail-oauth.wzldq.com/auth?a=1&amp;b=2">www.golia.jp</a>"#
            + #" and <a href='https://fine.example/'>fine</a></p>"#
        let marked = LinkWarnings.mark(html: html, warnings: [shows, listed]) { _ in "why \"so\"" }
        #expect(marked.targets.count == 1)
        let target = try #require(marked.targets.first)
        #expect(target.url.absoluteString == "https://webmail-oauth.wzldq.com/auth?a=1&b=2")
        #expect(target.warning == shows)
        #expect(!marked.html.contains("wzldq"))
        #expect(marked.html.contains(#"<a href="mailrs-link-warning:0">www.golia.jp</a>"#))
        #expect(marked.html.contains(#"aria-label="why &quot;so&quot;""#))
        // the plain link is left exactly as it was
        #expect(marked.html.contains(#"<a href='https://fine.example/'>fine</a>"#))
        #expect(LinkWarnings.index(of: URL(string: "mailrs-link-warning:0")!) == 0)
        #expect(LinkWarnings.index(of: URL(string: "https://fine.example/")!) == nil)
    }

    @Test("with no warnings the body is untouched")
    func untouched() {
        let html = #"<a href="https://bad.example/">x</a>"#
        #expect(LinkWarnings.mark(html: html, warnings: []) { _ in "" }.html == html)
        #expect(LinkWarnings.mark(html: html, warnings: [shows]) { _ in "" }.html == html)
    }

    @Test("a plain-text body marks a listed link and leaves the rest")
    func plainText() {
        let text = PlainTextLinks.attributed("go https://bad.example/x or https://fine.example/")
        let marked = String(LinkWarningMarks.marked(text, warnings: [listed, shows]).characters)
        #expect(marked == "go https://bad.example/x ! or https://fine.example/")
        #expect(LinkWarningMarks.warning(for: URL(string: "https://bad.example/x")!, in: [listed]) == listed)
        #expect(LinkWarningMarks.warning(for: URL(string: "https://fine.example/")!, in: [listed]) == nil)
    }
}

@Suite("Callback numbers")
struct CallbackNumbersTests {
    private func message(_ uid: UInt32, _ sender: String, _ text: String) -> Wire.Message {
        Wire.Message(
            uid: uid, sender: sender, senderTrust: "", recipients: "me", subject: "s",
            internalDate: 1, messageId: "<\(uid)>", textBody: text, htmlBody: nil,
            attachments: [], unsubscribe: nil)
    }

    private struct Sample: Decodable {
        let name: String
        let text: String
        let numbers: [String]
    }

    private struct Shared: Decodable {
        let labels: [String]
        let samples: [Sample]
    }

    /// `callback-numbers.json`, the web's table, as the app ships it.
    private func shared() throws -> Shared {
        let url = try #require(
            Bundle.main.url(forResource: "callback-numbers", withExtension: "json"),
            "callback-numbers.json is not in the app")
        return try JSONDecoder().decode(Shared.self, from: Data(contentsOf: url))
    }

    @Test("every shared sample reads as it does on the web")
    func sharedSamples() throws {
        let file = try shared()
        #expect(file.samples.count >= 10, "there are no samples to check")
        #expect(CallbackNumbers.labels == file.labels)
        for sample in file.samples {
            #expect(CallbackNumbers.phoneNumbers(in: sample.text) == sample.numbers, "\(sample.name)")
        }
    }

    @Test("a purchase order number is not joined to the invoice's year after it")
    func purchaseOrder() {
        let m = Wire.Message(
            uid: 1, sender: "billing@vendor.example", senderTrust: "", recipients: "me", subject: "s",
            internalDate: 1, messageId: "<1>", textBody: nil,
            htmlBody: "<table><tr><td>Invoice for Phase 1: Under PO 4300078149</td>\n<td>2026-INV-003001-2</td></tr></table>",
            attachments: [], unsubscribe: nil)
        #expect(CallbackNumbers.new(in: m, earlier: []) == [])
    }

    @Test("grouped numbers are found, account numbers and dates are not")
    func found() {
        let text = "Call +81 3-1234-5678 or (03) 1234 5678. Order 12345678901. On 2026-11-01 12.30.45"
        #expect(CallbackNumbers.phoneNumbers(in: text) == ["+81 3-1234-5678", "(03) 1234 5678"])
    }

    @Test("a number the same sender gave before is not pointed out")
    func earlier() {
        let first = message(1, "Bank <a@bank.example>", "Desk: 03-1234-5678")
        let later = message(2, "bank <A@bank.example>", "Call 03-1234-5678 or 0120-111-222")
        #expect(CallbackNumbers.new(in: later, earlier: [first]) == ["0120-111-222"])
        // from someone else, the number is new to this sender
        let other = message(3, "x@other.example", "Call 03-1234-5678")
        #expect(CallbackNumbers.new(in: other, earlier: [first]) == ["03-1234-5678"])
    }

    private func given(_ number: String, first: String) -> Wire.GivenNumber {
        Wire.GivenNumber(number: number, firstSeen: 1, firstMessageId: first)
    }

    @Test("a number the sender gave in another conversation is not pointed out")
    func history() {
        let m = message(2, "desk@hotel.example", "Call 03-1234-5678 or 0120-111-222")
        let elsewhere = [given("0120111222", first: "<older@hotel.example>")]
        #expect(CallbackNumbers.new(in: m, earlier: [], history: elsewhere) == ["03-1234-5678"])
    }

    @Test("a number first given by this very message is still new")
    func historyOfThisMessage() {
        let m = message(2, "desk@hotel.example", "Call +81 3-1234-5678")
        let own = [given("81312345678", first: "<2>")]
        #expect(CallbackNumbers.new(in: m, earlier: [], history: own) == ["+81 3-1234-5678"])
    }

    @Test("the history is compared by digits, however the number is written")
    func historyDigits() {
        let m = message(2, "desk@hotel.example", "Call (03) 1234 5678 or 03.9999.0000")
        let elsewhere = [given("0312345678", first: "<older@hotel.example>")]
        #expect(CallbackNumbers.new(in: m, earlier: [], history: elsewhere) == ["03.9999.0000"])
    }

    @Test("the history answer decodes as the server sends it")
    func historyDecodes() throws {
        let json = #"{"sender":"desk@hotel.example","numbers":[{"number":"0312345678","first_seen":1790000000,"first_message_id":"<first@hotel.example>"}]}"#
        let answer = try JSONDecoder().decode(Wire.CallbackHistory.self, from: Data(json.utf8))
        #expect(answer.numbers == [
            Wire.GivenNumber(number: "0312345678", firstSeen: 1790000000, firstMessageId: "<first@hotel.example>"),
        ])
    }

    @Test("at most three are named")
    func capped() {
        let m = message(1, "x@y", "a 03-1111-1111 b 03-2222-2222 c 03-3333-3333 d 03-4444-4444")
        #expect(CallbackNumbers.new(in: m, earlier: []).count == 3)
    }

    @Test("an HTML-only message is read as text")
    func html() {
        let m = Wire.Message(
            uid: 1, sender: "x@y", senderTrust: "", recipients: "me", subject: "s",
            internalDate: 1, messageId: "<1>", textBody: nil,
            htmlBody: "<p>Call <b>+81 3-9999-0000</b></p>", attachments: [], unsubscribe: nil)
        #expect(CallbackNumbers.new(in: m, earlier: []) == ["+81 3-9999-0000"])
    }
}
