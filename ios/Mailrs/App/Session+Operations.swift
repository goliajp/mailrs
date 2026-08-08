import Foundation
import SwiftUI

/// Running it — queue, DMARC, audit, API keys.
///
/// Split out of `Session.swift` at the 500-line limit this
/// repository holds every language to — and which did not look at
/// `ios/` until now. One type, several subjects.
@MainActor
extension Session {

    func queue() async throws -> [Wire.QueueJob] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.queue()
    }


    func suppressions() async throws -> [String] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.suppressions()
    }


    func clearSuppressions() async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.clearSuppressions()
    }


    func dmarcReports() async throws -> [Wire.DmarcReport] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.dmarcReports()
    }


    func dmarcSources() async throws -> Wire.DmarcSourceList {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.dmarcSources()
    }


    func auditLog(actionPrefix: String?) async throws -> [Wire.AuditRow] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.auditLog(actionPrefix: actionPrefix)
    }


    // MARK: Agent keys

    func agentKeys() async throws -> [Wire.AgentKey] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.agentKeys()
    }


    func createAgentKey(name: String, scopes: [String]) async throws -> Wire.CreateAgentKeyResponse {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.createAgentKey(name: name, scopes: scopes)
    }


    func deleteAgentKey(id: Int64) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.deleteAgentKey(id: id)
    }
}
