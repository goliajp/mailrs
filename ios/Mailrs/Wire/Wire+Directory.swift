import Foundation

/// Who exists — accounts, aliases, domains, groups and their permissions.
///
/// Split out of `Wire.swift` when it passed the 500-line limit this
/// repository holds every language to. Same namespace, same shapes —
/// only the file they live in changed.
extension Wire {

    /// Backend: `crates/core-api/src/method/admin/directory.rs` —
    /// `AliasWire`, served by `crates/webapi/src/handlers/admin_directory.rs`.
    ///
    /// `GET /api/admin/aliases` answers `{"items": [...]}` — an
    /// envelope, unlike `/api/conversations`, which is a bare array.
    /// The two shapes live in one app, so the difference is worth
    /// stating rather than remembering.
    struct Alias: Codable, Equatable, Identifiable, Sendable {
        let id: Int64
        let sourceAddress: String
        let targetAddress: String
        let domain: String
        /// `alias` or `forward` — the server's word, shown as-is.
        let aliasType: String
        let active: Bool
        let createdAt: Int64

        enum CodingKeys: String, CodingKey {
            case id
            case sourceAddress = "source_address"
            case targetAddress = "target_address"
            case domain
            case aliasType = "alias_type"
            case active
            case createdAt = "created_at"
        }
    }


    struct AliasList: Decodable, Sendable {
        let items: [Alias]
    }


    /// Backend: `AddAliasRequest`. `domain` is sent even though the
    /// server could split it off the source: the handler takes it as a
    /// field, and inferring what a server asks for is how a client
    /// starts disagreeing with it.
    struct AddAliasRequest: Encodable, Sendable {
        let sourceAddress: String
        let targetAddress: String
        let domain: String
        let aliasType: String

        enum CodingKeys: String, CodingKey {
            case sourceAddress = "source_address"
            case targetAddress = "target_address"
            case domain
            case aliasType = "alias_type"
        }
    }


    /// Backend: `crates/core-api/src/method/admin/directory.rs` —
    /// `AccountWire`. Same `{items: […]}` envelope as the alias list.
    struct Account: Codable, Equatable, Identifiable, Sendable {
        let address: String
        let domain: String
        let displayName: String
        let active: Bool
        let createdAt: Int64
        let quotaBytes: Int64
        let recoveryEmail: String?

        var id: String { address }

        enum CodingKeys: String, CodingKey {
            case address
            case domain
            case displayName = "display_name"
            case active
            case createdAt = "created_at"
            case quotaBytes = "quota_bytes"
            case recoveryEmail = "recovery_email"
        }
    }


    struct AccountList: Decodable, Sendable {
        let items: [Account]
    }


    /// Backend: `AddAccountRequest`. The password travels in plaintext
    /// over TLS and the server hashes it with Argon2 — so it is held
    /// only for the length of the request, never cached, never logged,
    /// and never written to a draft.
    struct AddAccountRequest: Encodable, Sendable {
        let address: String
        let displayName: String
        let password: String

        enum CodingKeys: String, CodingKey {
            case address
            case displayName = "display_name"
            case password
        }
    }


    /// Backend: `DomainWire`.
    struct Domain: Codable, Equatable, Identifiable, Sendable {
        let name: String
        let createdAt: Int64

        var id: String { name }

        enum CodingKeys: String, CodingKey {
            case name
            case createdAt = "created_at"
        }
    }


    struct DomainList: Decodable, Sendable {
        let items: [Domain]
    }


    struct AddDomainRequest: Encodable, Sendable {
        let name: String
    }


    /// Backend: `crates/core-api/src/method/admin/directory.rs` —
    /// `EmailGroupWire`. One address that delivers to many people,
    /// where an alias delivers to one.
    struct EmailGroup: Codable, Equatable, Identifiable, Sendable {
        let id: Int64
        let address: String
        let domain: String
        let name: String
        let description: String
        let createdAt: Int64

        enum CodingKeys: String, CodingKey {
            case id
            case address
            case domain
            case name
            case description
            case createdAt = "created_at"
        }
    }


    struct EmailGroupList: Decodable, Sendable {
        let items: [EmailGroup]
    }


    /// Backend: `EmailGroupMembersResponse` — bare addresses under
    /// `members`, not objects. The list endpoints in this area use
    /// `items`; this one does not, which is worth saying rather than
    /// discovering.
    struct EmailGroupMembers: Decodable, Sendable {
        let members: [String]
    }


    struct CreateEmailGroupRequest: Encodable, Sendable {
        let address: String
        let domain: String
        let name: String
        let description: String
    }


    struct EmailGroupMemberRequest: Encodable, Sendable {
        let memberAddress: String

        enum CodingKeys: String, CodingKey {
            case memberAddress = "member_address"
        }
    }


    /// Backend: `crates/core-api/src/method/admin/permissions.rs` —
    /// `GroupWire`. A permission group, not an email group: this one
    /// decides who may do things, the other decides where mail goes.
    struct PermissionGroup: Decodable, Identifiable, Sendable {
        let id: Int64
        let name: String
        /// Absent for the cross-domain builtins.
        let domain: String?
        let description: String
        let isBuiltin: Bool
        let createdAt: Int64

        enum CodingKeys: String, CodingKey {
            case id
            case name
            case domain
            case description
            case isBuiltin = "is_builtin"
            case createdAt = "created_at"
        }
    }


    struct PermissionGroupList: Decodable, Sendable {
        let items: [PermissionGroup]
    }


    /// Both the group's grants and the server's catalogue arrive under
    /// `permissions` — the same key for two different questions, which
    /// is worth naming rather than reusing one type for.
    struct PermissionSet: Decodable, Sendable {
        let permissions: [String]
    }


    struct GroupMembers: Decodable, Sendable {
        let members: [String]
    }


    struct SetPermissionsRequest: Encodable, Sendable {
        let permissions: [String]
    }


    struct AddGroupRequest: Encodable, Sendable {
        let name: String
        let domain: String?
        let description: String
    }
}
