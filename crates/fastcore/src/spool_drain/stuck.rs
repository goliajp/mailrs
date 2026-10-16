/// Filenames already reported as undeliverable. A spool file that
/// cannot be resolved stays on disk by design, so without this every
/// drain tick would re-log it forever.
static STUCK_REPORTED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// `true` the first time `filename` is seen as stuck. Also drops names
/// that have since left the spool, so a file that is fixed and then
/// breaks again is reported afresh.
pub(super) fn stuck_is_new(filename: &str) -> bool {
    match STUCK_REPORTED.lock() {
        Ok(mut set) => set.insert(filename.to_string()),
        // A poisoned mutex must not silence the warning.
        Err(_) => true,
    }
}

/// Forget any reported name no longer present in the spool.
pub(super) fn forget_departed(present: &std::collections::HashSet<String>) {
    if let Ok(mut set) = STUCK_REPORTED.lock() {
        set.retain(|name| present.contains(name));
    }
}
