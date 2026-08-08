import Foundation

/// Who exists: accounts, aliases, domains, groups, permissions.
///
/// Split out of `MailrsClient.swift` at the 500-line limit this
/// repository holds every language to — and which did not look at
/// `ios/` until now. One type, several subjects.
extension MailrsClient {

    /// `GET /api/admin/aliases` — Backend:
    /// `crates/webapi/src/handlers/admin_directory.rs::list_aliases`.
    func aliases() async throws -> [Wire.Alias] {
        let list: Wire.AliasList = try await getJSON("/api/admin/aliases")
        return list.items
    }


    /// `POST /api/admin/aliases`.
    func addAlias(_ request: Wire.AddAliasRequest) async throws {
        let (_, response) = try await send(
            "POST", "/api/admin/aliases",
            body: try JSONEncoder().encode(request), authorized: true
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }


    /// `DELETE /api/admin/aliases/{id}`.
    func deleteAlias(id: Int64) async throws {
        try await verb("DELETE", "/api/admin/aliases/\(id)")
    }


    /// `GET /api/admin/accounts`.
    func accounts() async throws -> [Wire.Account] {
        let list: Wire.AccountList = try await getJSON("/api/admin/accounts")
        return list.items
    }


    /// `POST /api/admin/accounts`.
    func addAccount(_ request: Wire.AddAccountRequest) async throws {
        let (_, response) = try await send(
            "POST", "/api/admin/accounts",
            body: try JSONEncoder().encode(request), authorized: true
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }


    /// `DELETE /api/admin/accounts/{address}`.
    func deleteAccount(address: String) async throws {
        let encoded = address.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? address
        try await verb("DELETE", "/api/admin/accounts/\(encoded)")
    }


    /// `GET /api/admin/domains`.
    func domains() async throws -> [Wire.Domain] {
        let list: Wire.DomainList = try await getJSON("/api/admin/domains")
        return list.items
    }


    /// `POST /api/admin/domains`.
    func addDomain(name: String) async throws {
        let (_, response) = try await send(
            "POST", "/api/admin/domains",
            body: try JSONEncoder().encode(Wire.AddDomainRequest(name: name)), authorized: true
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }


    /// `DELETE /api/admin/domains/{name}`.
    func deleteDomain(name: String) async throws {
        let encoded = name.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? name
        try await verb("DELETE", "/api/admin/domains/\(encoded)")
    }


    /// `GET /api/admin/email-groups`.
    func emailGroups() async throws -> [Wire.EmailGroup] {
        let list: Wire.EmailGroupList = try await getJSON("/api/admin/email-groups")
        return list.items
    }


    /// `POST /api/admin/email-groups`.
    func createEmailGroup(_ request: Wire.CreateEmailGroupRequest) async throws {
        let (_, response) = try await send(
            "POST", "/api/admin/email-groups",
            body: try JSONEncoder().encode(request), authorized: true
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }


    /// `DELETE /api/admin/email-groups/{id}`.
    func deleteEmailGroup(id: Int64) async throws {
        try await verb("DELETE", "/api/admin/email-groups/\(id)")
    }


    /// `GET /api/admin/email-groups/{id}/members` — `{members: [...]}`.
    func emailGroupMembers(id: Int64) async throws -> [String] {
        let list: Wire.EmailGroupMembers =
            try await getJSON("/api/admin/email-groups/\(id)/members")
        return list.members
    }


    /// `POST /api/admin/email-groups/{id}/members`.
    func addEmailGroupMember(id: Int64, address: String) async throws {
        let body = try JSONEncoder().encode(Wire.EmailGroupMemberRequest(memberAddress: address))
        let (_, response) = try await send(
            "POST", "/api/admin/email-groups/\(id)/members", body: body, authorized: true
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }


    /// `DELETE /api/admin/email-groups/{id}/members/{address}`.
    func removeEmailGroupMember(id: Int64, address: String) async throws {
        let encoded = address.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? address
        try await verb("DELETE", "/api/admin/email-groups/\(id)/members/\(encoded)")
    }


    /// `GET /api/admin/groups`.
    func permissionGroups() async throws -> [Wire.PermissionGroup] {
        let list: Wire.PermissionGroupList = try await getJSON("/api/admin/groups")
        return list.items
    }


    /// `POST /api/admin/groups`.
    func createPermissionGroup(_ request: Wire.AddGroupRequest) async throws {
        let (_, response) = try await send(
            "POST", "/api/admin/groups",
            body: try JSONEncoder().encode(request), authorized: true
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }


    /// `DELETE /api/admin/groups/{id}`.
    func deletePermissionGroup(id: Int64) async throws {
        try await verb("DELETE", "/api/admin/groups/\(id)")
    }


    /// `GET /api/admin/permissions` — the whole catalogue the server
    /// recognises, so the screen offers what exists rather than what
    /// someone remembered when writing it.
    func permissionCatalogue() async throws -> [String] {
        let set: Wire.PermissionSet = try await getJSON("/api/admin/permissions")
        return set.permissions
    }


    /// `GET /api/admin/groups/{id}/permissions`.
    func groupPermissions(id: Int64) async throws -> [String] {
        let set: Wire.PermissionSet = try await getJSON("/api/admin/groups/\(id)/permissions")
        return set.permissions
    }


    /// `PUT /api/admin/groups/{id}/permissions` — the whole set, not a
    /// delta: the handler replaces, so sending one permission grants
    /// exactly that one and revokes the rest.
    func setGroupPermissions(id: Int64, permissions: [String]) async throws {
        let body = try JSONEncoder().encode(Wire.SetPermissionsRequest(permissions: permissions))
        let (_, response) = try await send(
            "PUT", "/api/admin/groups/\(id)/permissions", body: body, authorized: true
        )
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }


    /// `GET /api/admin/groups/{id}/members`.
    func groupMembers(id: Int64) async throws -> [String] {
        let list: Wire.GroupMembers = try await getJSON("/api/admin/groups/\(id)/members")
        return list.members
    }
}
