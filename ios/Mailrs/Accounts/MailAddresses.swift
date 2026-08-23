import Foundation

/// Reading an address list out of a header.
///
/// The whole of the difficulty is one character. `To:` is
/// comma-separated, and a display name may contain a comma — which is
/// why it is quoted. Splitting on every comma turns
/// `"Lovelace, Ada" <ada@example.com>` into two recipients, one of
/// them nonsense, and a reply-all then sends to an address that does
/// not exist.
enum MailAddresses {
    /// The entries of a header, still with their display names.
    static func split(_ header: String) -> [String] {
        var out: [String] = []
        var current = ""
        var quoted = false
        var angled = false
        for ch in header {
            if ch == "\"" {
                quoted.toggle()
                current.append(ch)
                continue
            }
            // A comma inside `<...>` is not a separator either: a route
            // address (`<@a,@b:c@d>`) is obsolete but legal, and
            // splitting one produces two broken halves.
            if ch == "<", !quoted {
                angled = true
                current.append(ch)
                continue
            }
            if ch == ">", !quoted {
                angled = false
                current.append(ch)
                continue
            }
            if ch == ",", !quoted, !angled {
                out.append(current.trimmingCharacters(in: .whitespaces))
                current = ""
                continue
            }
            current.append(ch)
        }
        out.append(current.trimmingCharacters(in: .whitespaces))
        return out.filter { !$0.isEmpty }
    }

    /// The address itself, without the display name.
    ///
    /// For comparing, never for showing: `Ada <a@b>` and `a@b` are the
    /// same person, and a reply-all that does not know it copies
    /// somebody to their own message.
    static func bare(_ entry: String) -> String {
        guard let open = entry.lastIndex(of: "<"), let close = entry.lastIndex(of: ">"),
            open < close
        else { return entry.trimmingCharacters(in: .whitespaces).lowercased() }
        return entry[entry.index(after: open)..<close]
            .trimmingCharacters(in: .whitespaces).lowercased()
    }

    /// Everyone to copy on a reply-all, in the order they were
    /// written.
    ///
    /// Two rules, and both are about not annoying people: **the
    /// sender's own address never appears**, or every reply-all copies
    /// its author, and **nobody appears twice**, or somebody on both To
    /// and Cc gets two.
    static func replyAll(to: String, cc: String, primary: String, mine: String) -> [String] {
        var skip: Set<String> = [bare(primary), bare(mine)]
        var out: [String] = []
        for entry in split(to) + split(cc) {
            let key = bare(entry)
            if key.isEmpty || skip.contains(key) { continue }
            skip.insert(key)
            out.append(entry)
        }
        return out
    }
}
