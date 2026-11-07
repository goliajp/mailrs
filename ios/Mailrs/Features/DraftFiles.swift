import Foundation
import Observation

/// The files in a composer that keeps them with its draft.
///
/// A file is uploaded the moment it is added; once the server has it the
/// draft names it (`stored`). One the server refused stays as a file with
/// the server's reason and goes in the send's form, as one still uploading
/// does when the send comes first.
@Observable
@MainActor
final class DraftFiles {
    struct Entry: Identifiable {
        enum State {
            case uploading(MultipartForm.FilePart)
            case stored(Wire.DraftAttachment)
            case refused(MultipartForm.FilePart, reason: String)
        }

        // not the blob id: the same file can be attached twice
        let id = UUID()
        var state: State

        var name: String {
            switch state {
            case let .uploading(part), let .refused(part, _): return part.filename
            case let .stored(file): return file.name
            }
        }

        var size: Int64 {
            switch state {
            case let .uploading(part), let .refused(part, _): return Int64(part.data.count)
            case let .stored(file): return file.size
            }
        }

        var refusal: String? {
            if case let .refused(_, reason) = state { return reason }
            return nil
        }

        var isUploading: Bool {
            if case .uploading = state { return true }
            return false
        }
    }

    private(set) var entries: [Entry] = []
    /// The files the server's draft names, as of the last save that landed.
    private var savedKey = ""

    var stored: [Wire.DraftAttachment] {
        entries.compactMap { entry in
            if case let .stored(file) = entry.state { return file }
            return nil
        }
    }

    /// The files the server does not have; these go in the send's form.
    var unsaved: [MultipartForm.FilePart] {
        entries.compactMap { entry in
            switch entry.state {
            case let .uploading(part), let .refused(part, _): return part
            case .stored: return nil
            }
        }
    }

    /// The draft on the server names other files than the ones shown.
    var needsSave: Bool { Self.key(stored) != savedKey }

    var isEmpty: Bool { entries.isEmpty }

    /// Upload `part`; the entry becomes stored, or refused with the reason.
    func add(
        _ part: MultipartForm.FilePart,
        upload: @MainActor (MultipartForm.FilePart) async throws -> Wire.DraftAttachment
    ) async {
        let entry = Entry(state: .uploading(part))
        entries.append(entry)
        let next: Entry.State
        do {
            next = .stored(try await upload(part))
        } catch {
            next = .refused(part, reason: error.localizedDescription)
        }
        // a file removed while it uploaded stays removed
        guard let index = entries.firstIndex(where: { $0.id == entry.id }) else { return }
        entries[index].state = next
    }

    func remove(_ id: UUID) {
        entries.removeAll { $0.id == id }
    }

    /// The files of a draft the composer reopened: what the server names.
    func restore(_ files: [Wire.DraftAttachment]) {
        entries = files.map { Entry(state: .stored($0)) }
        savedKey = Self.key(files)
    }

    /// A save naming `files` landed.
    func saved(_ files: [Wire.DraftAttachment]) {
        savedKey = Self.key(files)
    }

    private static func key(_ files: [Wire.DraftAttachment]) -> String {
        files.map(\.blobId).joined(separator: ",")
    }
}
