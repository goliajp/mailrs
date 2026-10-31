# mailrs-attachment-scan

Reads what an email attachment carries, from its structure alone: its name,
its first bytes, the directory of an archive, the stream names of an Office
file, the dictionary names of a PDF, the scripts in an HTML or SVG file, and
whether its hash is on the caller's list. It reports facts; it decides
nothing. What counts as suspicious, how much, and what to do about it are
rules the caller writes over these facts.

Nothing is decompressed, executed or rendered, and the detectors that scan
contents read at most a set number of bytes of a part.

```rust
use mailrs_attachment_scan::{Attachment, Inspector, Limits};

let inspector = Inspector::build(&[], Limits::default(), 0);
let facts = inspector.inspect(&[Attachment {
    name: "invoice.pdf.exe",
    declared_type: "application/pdf",
    bytes: b"MZ",
}]);
assert_eq!(facts[0].executable_name.as_deref(), Some(".exe"));
assert!(facts[0].double_extension.is_some());
```

Hashes of known-bad files and a hash allowlist come from the caller as
observations; an allowed file is left out of the facts, so no rule can fire
on it.
