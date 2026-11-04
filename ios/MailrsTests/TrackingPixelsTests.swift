import Foundation
import Testing

@testable import Mailrs

/// Held to `web/src/lib/tracker-samples.json` (copied beside this test and
/// kept the same bytes by `scripts/check-tracker-parity.sh`), which the web
/// and Android tests read too, so all three clients count a message alike.
@Suite("Trackers")
struct TrackingPixelsTests {
    private final class Anchor {}

    struct Sample: Decodable, Sendable, CustomTestStringConvertible {
        let name: String
        let html: String
        let trackers: Int
        var testDescription: String { name }
    }

    struct File: Decodable { let samples: [Sample] }

    static let samples: [Sample] = {
        guard let url = Bundle(for: Anchor.self).url(forResource: "tracker-samples", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let file = try? JSONDecoder().decode(File.self, from: data)
        else { return [] }
        return file.samples
    }()

    @Test func theSharedSamplesAreRead() {
        #expect(Self.samples.count > 10, "web/src/lib/tracker-samples.json was not read")
    }

    @Test(arguments: samples)
    func countsAsTheWebDoes(_ sample: Sample) {
        #expect(TrackingPixels.count(html: sample.html) == sample.trackers)
    }

    /// `strip` takes out exactly what `count` counts, and nothing else.
    @Test(arguments: samples)
    func stripsWhatItCounts(_ sample: Sample) {
        let stripped = TrackingPixels.strip(html: sample.html)
        #expect(TrackingPixels.count(html: stripped) == 0)
        if sample.trackers == 0 { #expect(stripped == sample.html) }
    }

    @Test("the shipped rules are the web's")
    func shipsTheRules() {
        #expect(TrackingPixels.rules.hosts.contains("mailtrack.io"))
        #expect(TrackingPixels.rules.paths.contains("/wf/open"))
    }
}
