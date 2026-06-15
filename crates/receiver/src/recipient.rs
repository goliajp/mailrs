/// Outcome of resolving a recipient address through aliases / groups /
/// forwards — the one domain type that crosses the receiver's account seam.
#[derive(Debug, Clone)]
pub enum ResolvedRecipient {
    /// A real local account; deliver to its mailbox.
    Account(String),
    /// Group email: deliver a copy to each member's mailbox.
    Group(Vec<String>),
    /// Alias forward: deliver / relay to each target address.
    Forward(Vec<String>),
    /// No matching account / alias; the caller decides the response.
    Reject,
}
