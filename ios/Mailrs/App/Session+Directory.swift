import Foundation
import SwiftUI

/// Who exists — the directory screens' half of the session.
///
/// Split out of `Session.swift` at the 500-line limit this
/// repository holds every language to — and which did not look at
/// `ios/` until now. One type, several subjects.
@MainActor
extension Session {

    // MARK: Administration

    func aliases() async throws -> [Wire.Alias] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.aliases()
    }


    func loadMyAliases() async {
        guard myAliases.isEmpty, let client else { return }
        // Swallowed on purpose, and the only swallow in this file with a
        // defensible silence: the endpoint is admin-adjacent, a user
        // without the permission gets a 403, and the honest consequence
        // is no marks — not an error banner about the directory on top
        // of somebody's mail.
        let all = (try? await client.aliases()) ?? []
        let mine = myAddress
        myAliases = all.filter { $0.targetAddress.lowercased() == mine }
    }


    /// The domain travels as its own field because the handler takes
    /// one, and `alias` is the type this screen creates — `forward` is
    /// a different feature with different semantics, not a spelling.
    func addAlias(source: String, target: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.addAlias(Wire.AddAliasRequest(
            sourceAddress: source,
            targetAddress: target,
            domain: AliasRule.domain(of: source),
            aliasType: "alias"
        ))
    }


    func deleteAlias(id: Int64) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.deleteAlias(id: id)
    }


    func accounts() async throws -> [Wire.Account] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.accounts()
    }


    /// The password is a parameter and nothing else: it is not stored
    /// on this object, not logged, and not carried into a retry.
    func addAccount(address: String, displayName: String, password: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.addAccount(Wire.AddAccountRequest(
            address: address, displayName: displayName, password: password
        ))
    }


    func deleteAccount(address: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.deleteAccount(address: address)
    }


    func domains() async throws -> [Wire.Domain] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.domains()
    }


    func addDomain(name: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.addDomain(name: name)
    }


    func deleteDomain(name: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.deleteDomain(name: name)
    }


    func emailGroups() async throws -> [Wire.EmailGroup] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.emailGroups()
    }


    func createEmailGroup(address: String, name: String, description: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.createEmailGroup(Wire.CreateEmailGroupRequest(
            address: address,
            domain: AliasRule.domain(of: address),
            name: name,
            description: description
        ))
    }


    func deleteEmailGroup(id: Int64) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.deleteEmailGroup(id: id)
    }


    func emailGroupMembers(id: Int64) async throws -> [String] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.emailGroupMembers(id: id)
    }


    func addEmailGroupMember(id: Int64, address: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.addEmailGroupMember(id: id, address: address)
    }


    func removeEmailGroupMember(id: Int64, address: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.removeEmailGroupMember(id: id, address: address)
    }


    func permissionGroups() async throws -> [Wire.PermissionGroup] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.permissionGroups()
    }


    func createPermissionGroup(name: String, domain: String?, description: String) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.createPermissionGroup(
            Wire.AddGroupRequest(name: name, domain: domain, description: description)
        )
    }


    func deletePermissionGroup(id: Int64) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.deletePermissionGroup(id: id)
    }


    func permissionCatalogue() async throws -> [String] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.permissionCatalogue()
    }


    func groupPermissions(id: Int64) async throws -> [String] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.groupPermissions(id: id)
    }


    func setGroupPermissions(id: Int64, permissions: [String]) async throws {
        guard let client else { throw MailrsError.badCredentials }
        try await client.setGroupPermissions(id: id, permissions: permissions)
    }


    func groupMembers(id: Int64) async throws -> [String] {
        guard let client else { throw MailrsError.badCredentials }
        return try await client.groupMembers(id: id)
    }
}
