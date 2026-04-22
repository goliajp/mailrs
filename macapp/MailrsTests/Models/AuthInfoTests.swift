import Testing
import Foundation
@testable import Mailrs

@Suite("AuthInfo + LoginResponse decoding")
struct AuthInfoTests {
    @Test("decodes a success login response (no TOTP)")
    func successResponse() throws {
        let json = """
        {
          "token": "abc123",
          "address": "lihao@golia.jp",
          "display_name": "Li Hao",
          "permissions": ["mail.read", "mail.send"],
          "accessible_domains": ["golia.jp"],
          "send_as": ["lihao@golia.jp", "admin@golia.jp"]
        }
        """.data(using: .utf8)!

        let res = try JSONCoders.decoder.decode(LoginResponseDTO.self, from: json)
        #expect(res.requires_totp == nil)
        #expect(res.token == "abc123")
        #expect(res.address == "lihao@golia.jp")
        #expect(res.display_name == "Li Hao")
        #expect(res.permissions == ["mail.read", "mail.send"])
        #expect(res.accessible_domains == ["golia.jp"])
    }

    @Test("decodes a totp-required response")
    func totpRequiredResponse() throws {
        let json = #"{"requires_totp": true}"#.data(using: .utf8)!

        let res = try JSONCoders.decoder.decode(LoginResponseDTO.self, from: json)
        #expect(res.requires_totp == true)
        #expect(res.token == nil)
    }

    @Test("AuthInfo round-trip preserves all fields")
    func authInfoRoundTrip() throws {
        let info = AuthInfo(
            token: "tok",
            address: "a@b.com",
            display_name: "Alice",
            permissions: ["admin.*"],
            accessible_domains: ["b.com"],
            send_as: nil
        )
        let data = try JSONCoders.encoder.encode(info)
        let decoded = try JSONCoders.decoder.decode(AuthInfo.self, from: data)
        #expect(decoded == info)
        #expect(decoded.sendAs == ["a@b.com"])
    }

    @Test("Unix-seconds date decoder handles Int and Double")
    func unixSecondsDecoder() throws {
        struct Wrap: Decodable { let created_at: Date }

        let asInt = try JSONCoders.decoder.decode(Wrap.self, from: Data(#"{"created_at": 1700000000}"#.utf8))
        #expect(asInt.created_at.timeIntervalSince1970 == 1700000000)

        let asDouble = try JSONCoders.decoder.decode(Wrap.self, from: Data(#"{"created_at": 1700000000.5}"#.utf8))
        #expect(asDouble.created_at.timeIntervalSince1970 == 1700000000.5)
    }
}
